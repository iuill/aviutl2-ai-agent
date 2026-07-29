use std::{
    fs::OpenOptions,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
use aviutl2_ai_agent_protocol::FrameRate;
use aviutl2_ai_agent_protocol::{
    ApiError, CurrentScene, CurrentTimeline, ErrorCode, Health, HealthStatus, Status,
};

use crate::editor::{EditorError, EditorGate};

const MAX_REQUEST_HEAD: usize = 8 * 1024;
const IO_TIMEOUT: Duration = Duration::from_millis(250);
const ACCEPT_POLL: Duration = Duration::from_millis(5);
const EDITOR_GATE_TIMEOUT: Duration = Duration::from_millis(100);
const RETRY_AFTER_SECONDS: u64 = 1;
const API_VERSION: &str = "v1";
const SCENE_OBSERVATION_LOG_ENV: &str = "AVIUTL2_AI_AGENT_SCENE_OBSERVATION_LOG";
#[cfg(windows)]
const OBJECT_OBSERVATION_LOG_ENV: &str = "AVIUTL2_AI_AGENT_OBJECT_OBSERVATION_LOG";

#[derive(Debug, Clone, PartialEq, Eq)]
struct SceneRead {
    name: String,
    raw_scene_id: Option<i32>,
}

type SceneReader = dyn Fn() -> Result<SceneRead, EditorError> + Send + Sync;
type TimelineReader = dyn Fn() -> Result<CurrentTimeline, EditorError> + Send + Sync;

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("worker_count must be greater than zero")]
    NoWorkers,
    #[error("failed to bind API server: {0}")]
    Bind(#[source] std::io::Error),
    #[error("failed to configure API listener: {0}")]
    Configure(#[source] std::io::Error),
    #[error("failed to spawn API worker: {0}")]
    Spawn(#[source] std::io::Error),
}

struct ServerContext {
    status: Status,
    expected_host: String,
    editor_gate: EditorGate,
    scene_reader: Arc<SceneReader>,
    timeline_reader: Arc<TimelineReader>,
}

/// Loopback HTTP server whose threads and SDK access gate are owned here.
///
/// Every response closes its connection. This prevents an idle keep-alive task
/// from executing plugin code after AviUtl2 unloads the DLL.
pub struct ApiServer {
    address: SocketAddr,
    shutting_down: Arc<AtomicBool>,
    workers: Vec<JoinHandle<()>>,
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct ShutdownObservation {
    pub(crate) worker_count: usize,
    pub(crate) join_panics: usize,
}

impl ApiServer {
    pub fn start(address: &str, worker_count: usize) -> Result<Self, ServerError> {
        Self::start_with_parts(
            address,
            worker_count,
            platform_scene_reader(),
            platform_timeline_reader(),
            |index, listener, shutting_down, context| {
                thread::Builder::new()
                    .name(format!("aviutl2-ai-agent-http-{index}"))
                    .spawn(move || worker_loop(listener, shutting_down, context))
            },
        )
    }

    fn start_with_parts<F>(
        address: &str,
        worker_count: usize,
        scene_reader: Arc<SceneReader>,
        timeline_reader: Arc<TimelineReader>,
        mut spawn_worker: F,
    ) -> Result<Self, ServerError>
    where
        F: FnMut(
            usize,
            Arc<TcpListener>,
            Arc<AtomicBool>,
            Arc<ServerContext>,
        ) -> std::io::Result<JoinHandle<()>>,
    {
        if worker_count == 0 {
            return Err(ServerError::NoWorkers);
        }

        let listener = TcpListener::bind(address).map_err(ServerError::Bind)?;
        listener
            .set_nonblocking(true)
            .map_err(ServerError::Configure)?;
        let address = listener.local_addr().map_err(ServerError::Configure)?;
        let listener = Arc::new(listener);
        let shutting_down = Arc::new(AtomicBool::new(false));
        let context = Arc::new(ServerContext {
            status: Status {
                status: HealthStatus::Ok,
                plugin_version: env!("CARGO_PKG_VERSION").to_owned(),
                api_version: API_VERSION.to_owned(),
                listener_address: address.to_string(),
                process_id: std::process::id(),
            },
            expected_host: address.to_string(),
            editor_gate: EditorGate::new(EDITOR_GATE_TIMEOUT),
            scene_reader,
            timeline_reader,
        });
        let mut workers: Vec<JoinHandle<()>> = Vec::with_capacity(worker_count);

        for index in 0..worker_count {
            let worker = match spawn_worker(
                index,
                Arc::clone(&listener),
                Arc::clone(&shutting_down),
                Arc::clone(&context),
            ) {
                Ok(worker) => worker,
                Err(error) => {
                    shutting_down.store(true, Ordering::Release);
                    for worker in workers.drain(..) {
                        let _ = worker.join();
                    }
                    return Err(ServerError::Spawn(error));
                }
            };
            workers.push(worker);
        }

        Ok(Self {
            address,
            shutting_down,
            workers,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.address
    }

    pub(crate) fn shutdown(&mut self) -> ShutdownObservation {
        self.shutting_down.store(true, Ordering::Release);
        let worker_count = self.workers.len();
        let mut join_panics = 0;
        for worker in self.workers.drain(..) {
            if worker.join().is_err() {
                join_panics += 1;
            }
        }
        ShutdownObservation {
            worker_count,
            join_panics,
        }
    }
}

impl Drop for ApiServer {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn worker_loop(
    listener: Arc<TcpListener>,
    shutting_down: Arc<AtomicBool>,
    context: Arc<ServerContext>,
) {
    while !shutting_down.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => handle_connection(stream, &context),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL);
            }
            Err(_) => break,
        }
    }
}

fn handle_connection(mut stream: TcpStream, context: &ServerContext) {
    if stream.set_read_timeout(Some(IO_TIMEOUT)).is_err() {
        return;
    }
    if stream.set_write_timeout(Some(IO_TIMEOUT)).is_err() {
        return;
    }
    let _ = stream.set_nodelay(true);

    let Some(request) = read_request_head(&mut stream) else {
        return;
    };
    let response = route(&request, context);

    let mut head = format!(
        "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        response.status,
        response.body.len()
    );
    if let Some(retry_after) = response.retry_after {
        head.push_str(&format!("Retry-After: {retry_after}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&response.body);
    let _ = stream.flush();
}

fn read_request_head(stream: &mut TcpStream) -> Option<String> {
    let mut request = Vec::with_capacity(512);
    let mut chunk = [0_u8; 512];
    while request.len() < MAX_REQUEST_HEAD {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(count) => {
                request.extend_from_slice(&chunk[..count]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    return std::str::from_utf8(&request).ok().map(str::to_owned);
                }
            }
        }
    }
    None
}

struct HttpResponse {
    status: &'static str,
    body: Vec<u8>,
    retry_after: Option<u64>,
}

fn route(request: &str, context: &ServerContext) -> HttpResponse {
    let mut lines = request.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut host = None;
    let mut has_origin = false;
    for line in lines.take_while(|line| !line.is_empty()) {
        let Some((name, value)) = line.split_once(':') else {
            return api_error(
                "400 Bad Request",
                ErrorCode::InvalidRequest,
                "Invalid request header",
                false,
                None,
            );
        };
        if name.eq_ignore_ascii_case("host") {
            host = Some(value.trim());
        } else if name.eq_ignore_ascii_case("origin") {
            has_origin = true;
        }
    }

    if host != Some(context.expected_host.as_str()) || has_origin {
        return api_error(
            "400 Bad Request",
            ErrorCode::InvalidRequest,
            "Request origin is not allowed",
            false,
            None,
        );
    }

    let mut parts = request_line.split_whitespace();
    let method = parts.next();
    let path = parts.next();
    let version = parts.next();
    if parts.next().is_some() || method != Some("GET") || version != Some("HTTP/1.1") {
        return api_error(
            "400 Bad Request",
            ErrorCode::InvalidRequest,
            "Invalid request line",
            false,
            None,
        );
    }

    match path {
        Some("/healthz") => json_response(&Health {
            status: HealthStatus::Ok,
            plugin_version: env!("CARGO_PKG_VERSION").to_owned(),
        }),
        Some("/v1/status") => json_response(&context.status),
        Some("/v1/scenes/current") => match context.editor_gate.read(|| (context.scene_reader)()) {
            Ok(scene) => {
                write_scene_observation(&scene);
                json_response(&CurrentScene { name: scene.name })
            }
            Err(EditorError::Busy) => api_error(
                "503 Service Unavailable",
                ErrorCode::EditorBusy,
                "EditorGate is busy",
                true,
                Some(RETRY_AFTER_SECONDS),
            ),
            Err(EditorError::Unavailable) => api_error(
                "503 Service Unavailable",
                ErrorCode::EditorUnavailable,
                "AviUtl2 did not accept the read",
                true,
                Some(RETRY_AFTER_SECONDS),
            ),
        },
        Some("/v1/scenes/current/timeline") => {
            match context.editor_gate.read(|| (context.timeline_reader)()) {
                Ok(timeline) => json_response(&timeline),
                Err(EditorError::Busy) => api_error(
                    "503 Service Unavailable",
                    ErrorCode::EditorBusy,
                    "EditorGate is busy",
                    true,
                    Some(RETRY_AFTER_SECONDS),
                ),
                Err(EditorError::Unavailable) => api_error(
                    "503 Service Unavailable",
                    ErrorCode::EditorUnavailable,
                    "AviUtl2 did not accept the read",
                    true,
                    Some(RETRY_AFTER_SECONDS),
                ),
            }
        }
        _ => api_error(
            "404 Not Found",
            ErrorCode::RouteNotFound,
            "Route not found",
            false,
            None,
        ),
    }
}

fn json_response(value: &impl serde::Serialize) -> HttpResponse {
    match serde_json::to_vec(value) {
        Ok(body) => HttpResponse {
            status: "200 OK",
            body,
            retry_after: None,
        },
        Err(_) => api_error(
            "500 Internal Server Error",
            ErrorCode::InternalError,
            "Plugin internal error",
            false,
            None,
        ),
    }
}

fn api_error(
    status: &'static str,
    code: ErrorCode,
    message: &str,
    retryable: bool,
    retry_after: Option<u64>,
) -> HttpResponse {
    let body = serde_json::to_vec(&ApiError {
        code,
        message: message.to_owned(),
        retryable,
    })
    .expect("ApiError is serializable");
    HttpResponse {
        status,
        body,
        retry_after,
    }
}

fn write_scene_observation(scene: &SceneRead) {
    let Ok(path) = std::env::var(SCENE_OBSERVATION_LOG_ENV) else {
        return;
    };
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let timestamp_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let record = serde_json::json!({
        "timestampMillis": timestamp_millis,
        "name": scene.name,
        "rawSceneId": scene.raw_scene_id,
    });
    let _ = writeln!(file, "{record}");
}

#[cfg(windows)]
fn platform_scene_reader() -> Arc<SceneReader> {
    Arc::new(|| {
        if !crate::windows_plugin::EDIT_HANDLE.is_ready() {
            return Err(EditorError::Unavailable);
        }
        let raw_scene_id = crate::windows_plugin::EDIT_HANDLE.get_edit_info().scene_id;
        crate::windows_plugin::EDIT_HANDLE
            .call_read_section(|section| {
                section.get_scene_name().map(|name| SceneRead {
                    name,
                    raw_scene_id: Some(raw_scene_id),
                })
            })
            .ok()
            .and_then(Result::ok)
            .ok_or(EditorError::Unavailable)
    })
}

#[cfg(windows)]
fn platform_timeline_reader() -> Arc<TimelineReader> {
    Arc::new(|| {
        if !crate::windows_plugin::EDIT_HANDLE.is_ready() {
            return Err(EditorError::Unavailable);
        }
        let info = crate::windows_plugin::EDIT_HANDLE.get_edit_info();
        let _ = crate::windows_plugin::EDIT_HANDLE.call_read_section(|section| {
            write_object_observation(section, info.scene_id, info.layer_max)
        });
        Ok(CurrentTimeline {
            width: info.width,
            height: info.height,
            frame_rate: FrameRate {
                numerator: *info.fps.numer(),
                denominator: *info.fps.denom(),
            },
            cursor_frame: info.frame,
            object_end_frame: info.frame_max,
            highest_object_layer: info.layer_max,
        })
    })
}

#[cfg(windows)]
fn write_object_observation(
    section: &aviutl2::generic::ReadSection,
    raw_scene_id: i32,
    layer_max: usize,
) {
    let Ok(path) = std::env::var(OBJECT_OBSERVATION_LOG_ENV) else {
        return;
    };
    let mut objects = Vec::new();
    for layer in 0..=layer_max {
        for (position, handle) in section.objects_in_layer(layer) {
            objects.push(serde_json::json!({
                "handle": format!("{handle:?}"),
                "layer": position.layer,
                "start": position.start,
                "end": position.end,
                "name": section.get_object_name(handle).ok().flatten(),
            }));
        }
    }
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let timestamp_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let record = serde_json::json!({
        "timestampMillis": timestamp_millis,
        "rawSceneId": raw_scene_id,
        "objects": objects,
    });
    let _ = writeln!(file, "{record}");
}

#[cfg(not(windows))]
fn platform_scene_reader() -> Arc<SceneReader> {
    Arc::new(|| Err(EditorError::Unavailable))
}

#[cfg(not(windows))]
fn platform_timeline_reader() -> Arc<TimelineReader> {
    Arc::new(|| Err(EditorError::Unavailable))
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
        thread,
        time::{Duration, Instant},
    };

    use aviutl2_ai_agent_protocol::{
        ApiError, CurrentScene, ErrorCode, Health, HealthStatus, Status,
    };

    use super::{ACCEPT_POLL, ApiServer, EditorError, ServerError, handle_connection, worker_loop};

    fn start(
        reader: impl Fn() -> Result<super::SceneRead, EditorError> + Send + Sync + 'static,
    ) -> ApiServer {
        ApiServer::start_with_parts(
            "127.0.0.1:0",
            4,
            Arc::new(reader),
            Arc::new(|| Ok(timeline())),
            |_, listener, shutting_down, context| {
                thread::Builder::new().spawn(move || worker_loop(listener, shutting_down, context))
            },
        )
        .unwrap()
    }

    fn scene(name: &str, raw_scene_id: i32) -> super::SceneRead {
        super::SceneRead {
            name: name.to_owned(),
            raw_scene_id: Some(raw_scene_id),
        }
    }

    fn timeline() -> aviutl2_ai_agent_protocol::CurrentTimeline {
        aviutl2_ai_agent_protocol::CurrentTimeline {
            width: 1920,
            height: 1080,
            frame_rate: aviutl2_ai_agent_protocol::FrameRate {
                numerator: 30,
                denominator: 1,
            },
            cursor_frame: 0,
            object_end_frame: 0,
            highest_object_layer: 0,
        }
    }

    fn request(address: std::net::SocketAddr, path: &str) -> String {
        raw_request(
            address,
            &format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: keep-alive\r\n\r\n"),
        )
    }

    fn raw_request(address: std::net::SocketAddr, request: &str) -> String {
        let mut stream = TcpStream::connect(address).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    fn body(response: &str) -> &str {
        response.split_once("\r\n\r\n").unwrap().1
    }

    #[test]
    fn health_status_and_not_found_use_json_and_close_connection() {
        let server = start(|| Ok(scene("Root", 0)));

        let response = request(server.local_addr(), "/healthz");
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response.contains("\r\nConnection: close\r\n"));
        let health: Health = serde_json::from_str(body(&response)).unwrap();
        assert_eq!(health.status, HealthStatus::Ok);

        let response = request(server.local_addr(), "/v1/status");
        let status: Status = serde_json::from_str(body(&response)).unwrap();
        assert_eq!(status.api_version, "v1");
        assert_eq!(status.listener_address, server.local_addr().to_string());

        let response = request(server.local_addr(), "/missing");
        assert!(response.starts_with("HTTP/1.1 404 Not Found\r\n"));
        let error: ApiError = serde_json::from_str(body(&response)).unwrap();
        assert_eq!(error.code, ErrorCode::RouteNotFound);
        assert!(!error.retryable);
    }

    #[test]
    fn current_scene_uses_reader() {
        let server = start(|| Ok(scene("Scene 1", 7)));
        let response = request(server.local_addr(), "/v1/scenes/current");
        let scene: CurrentScene = serde_json::from_str(body(&response)).unwrap();
        assert_eq!(scene.name, "Scene 1");
    }

    #[test]
    fn current_timeline_uses_sdk_independent_dto() {
        let server = start(|| Ok(scene("Root", 0)));
        let response = request(server.local_addr(), "/v1/scenes/current/timeline");
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        let actual: aviutl2_ai_agent_protocol::CurrentTimeline =
            serde_json::from_str(body(&response)).unwrap();
        assert_eq!(actual, timeline());
    }

    #[test]
    fn rejects_wrong_host_origin_and_invalid_method() {
        let server = start(|| Ok(scene("Root", 0)));
        let address = server.local_addr();
        for request_head in [
            "GET /healthz HTTP/1.1\r\nHost: attacker.example\r\n\r\n".to_owned(),
            format!("GET /healthz HTTP/1.1\r\nHost: {address}\r\nOrigin: null\r\n\r\n"),
            format!("POST /healthz HTTP/1.1\r\nHost: {address}\r\n\r\n"),
            format!("GET /healthz HTTP/1.0\r\nHost: {address}\r\n\r\n"),
        ] {
            let response = raw_request(address, &request_head);
            assert!(response.starts_with("HTTP/1.1 400 Bad Request\r\n"));
            let error: ApiError = serde_json::from_str(body(&response)).unwrap();
            assert_eq!(error.code, ErrorCode::InvalidRequest);
            assert!(!error.retryable);
        }
    }

    #[test]
    fn busy_and_unavailable_are_retryable() {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = std::sync::Mutex::new(release_rx);
        let server = start(move || {
            entered_tx.send(()).unwrap();
            release_rx.lock().unwrap().recv().unwrap();
            Ok(scene("Root", 0))
        });
        let address = server.local_addr();
        let holder = thread::spawn(move || request(address, "/v1/scenes/current"));
        entered_rx.recv().unwrap();

        let response = request(server.local_addr(), "/v1/scenes/current");
        assert!(response.starts_with("HTTP/1.1 503 Service Unavailable\r\n"));
        assert!(response.contains("\r\nRetry-After: 1\r\n"));
        let error: ApiError = serde_json::from_str(body(&response)).unwrap();
        assert_eq!(error.code, ErrorCode::EditorBusy);
        assert!(error.retryable);

        release_tx.send(()).unwrap();
        let _ = holder.join().unwrap();

        let unavailable = start(|| Err(EditorError::Unavailable));
        let response = request(unavailable.local_addr(), "/v1/scenes/current");
        let error: ApiError = serde_json::from_str(body(&response)).unwrap();
        assert_eq!(error.code, ErrorCode::EditorUnavailable);
        assert!(error.retryable);
    }

    #[test]
    fn health_and_status_respond_while_editor_gate_is_held() {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = std::sync::Mutex::new(release_rx);
        let server = start(move || {
            entered_tx.send(()).unwrap();
            release_rx.lock().unwrap().recv().unwrap();
            Ok(scene("Root", 0))
        });
        let address = server.local_addr();
        let holder = thread::spawn(move || request(address, "/v1/scenes/current"));
        entered_rx.recv().unwrap();

        let started = Instant::now();
        assert!(request(server.local_addr(), "/healthz").starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(request(server.local_addr(), "/v1/status").starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(started.elapsed() < Duration::from_millis(100));

        release_tx.send(()).unwrap();
        let _ = holder.join().unwrap();
    }

    #[test]
    fn drop_joins_workers_and_releases_the_port() {
        let accepted = Arc::new(AtomicBool::new(false));
        let mut server = ApiServer::start_with_parts(
            "127.0.0.1:0",
            1,
            Arc::new(|| Ok(scene("Root", 0))),
            Arc::new(|| Ok(timeline())),
            {
                let accepted = Arc::clone(&accepted);
                move |_, listener, shutting_down, context| {
                    let accepted = Arc::clone(&accepted);
                    thread::Builder::new().spawn(move || {
                        while !shutting_down.load(Ordering::Acquire) {
                            match listener.accept() {
                                Ok((stream, _)) => {
                                    accepted.store(true, Ordering::Release);
                                    handle_connection(stream, &context);
                                }
                                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                    thread::sleep(ACCEPT_POLL);
                                }
                                Err(_) => break,
                            }
                        }
                    })
                }
            },
        )
        .unwrap();
        let address = server.local_addr();
        let mut idle_keep_alive = TcpStream::connect(address).unwrap();
        idle_keep_alive
            .write_all(b"GET /healthz HTTP/1.1\r\n")
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while !accepted.load(Ordering::Acquire) {
            assert!(
                Instant::now() < deadline,
                "worker did not accept connection"
            );
            thread::sleep(Duration::from_millis(1));
        }

        let started = Instant::now();
        let observation = server.shutdown();
        drop(idle_keep_alive);

        assert_eq!(observation.worker_count, 1);
        assert_eq!(observation.join_panics, 0);
        assert!(started.elapsed() < Duration::from_millis(500));
        TcpListener::bind(address).expect("listener must be released after drop");
    }

    #[test]
    fn zero_workers_is_rejected() {
        assert!(matches!(
            ApiServer::start("127.0.0.1:0", 0),
            Err(ServerError::NoWorkers)
        ));
    }

    #[test]
    fn spawn_failure_stops_started_workers_and_releases_port() {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = probe.local_addr().unwrap();
        drop(probe);

        let result = ApiServer::start_with_parts(
            &address.to_string(),
            4,
            Arc::new(|| Ok(scene("Root", 0))),
            Arc::new(|| Ok(timeline())),
            |index, listener, shutting_down, context| {
                if index == 2 {
                    return Err(std::io::Error::other("injected spawn failure"));
                }
                thread::Builder::new().spawn(move || worker_loop(listener, shutting_down, context))
            },
        );
        assert!(matches!(result, Err(ServerError::Spawn(_))));
        TcpListener::bind(address)
            .expect("startup rollback must stop workers and release listener");
    }
}
