use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use aviutl2_ai_agent_protocol::{
    ApiError, CreateMediaObjectRequest, CreateMediaObjectResponse, CreateTextObjectRequest,
    CreateTextObjectResponse, CurrentObjectDetails, CurrentObjects, CurrentScene, CurrentTimeline,
    DeleteObjectRequest, DeleteObjectResponse, DuplicateObjectRequest, DuplicateObjectResponse,
    ErrorCode, Health, HealthStatus, MoveObjectRequest, MoveObjectResponse, Status,
    UpdateTextObjectRequest, UpdateTextObjectResponse,
};
#[cfg(windows)]
use aviutl2_ai_agent_protocol::{FrameRate, ObjectDetails, ObjectKind, TimelineObject};

use crate::{
    editor::{EditorError, EditorGate},
    mutation::MoveValidationError,
};

const MAX_REQUEST_HEAD: usize = 8 * 1024;
const MAX_REQUEST_BODY: usize = 16 * 1024;
const IO_TIMEOUT: Duration = Duration::from_millis(250);
const EXPECT_BODY_TIMEOUT: Duration = Duration::from_secs(2);
const ACCEPT_POLL: Duration = Duration::from_millis(5);
const EDITOR_GATE_TIMEOUT: Duration = Duration::from_millis(100);
const RETRY_AFTER_SECONDS: u64 = 1;
const API_VERSION: &str = "v1";
const SCENE_OBSERVATION_LOG_ENV: &str = "AVIUTL2_AI_AGENT_SCENE_OBSERVATION_LOG";
const HTTP_DIAGNOSTIC_LOG_ENV: &str = "AVIUTL2_AI_AGENT_HTTP_DIAGNOSTIC_LOG";
#[cfg(windows)]
const OBJECT_OBSERVATION_LOG_ENV: &str = "AVIUTL2_AI_AGENT_OBJECT_OBSERVATION_LOG";
#[cfg(windows)]
const MUTATION_DEBUG_LOG_ENV: &str = "AVIUTL2_AI_AGENT_MUTATION_DEBUG_LOG";
#[cfg(windows)]
const TEXT_EFFECT_NAME: &str = "テキスト";
#[cfg(windows)]
const TEXT_ITEM_NAME: &str = "テキスト";
#[cfg(windows)]
const IMAGE_EFFECT_NAME: &str = "画像ファイル";
#[cfg(windows)]
const AUDIO_EFFECT_NAME: &str = "音声ファイル";

#[derive(Debug, Clone, PartialEq, Eq)]
struct SceneRead {
    name: String,
    raw_scene_id: Option<i32>,
}

type SceneReader = dyn Fn() -> Result<SceneRead, EditorError> + Send + Sync;
type TimelineReader = dyn Fn() -> Result<CurrentTimeline, EditorError> + Send + Sync;
type ObjectsReader = dyn Fn() -> Result<CurrentObjects, EditorError> + Send + Sync;
type ObjectDetailsReader = dyn Fn() -> Result<CurrentObjectDetails, EditorError> + Send + Sync;
type ObjectMover =
    dyn Fn(&MoveObjectRequest) -> Result<MoveObjectResponse, MutationError> + Send + Sync;
type ObjectDeleter =
    dyn Fn(&DeleteObjectRequest) -> Result<DeleteObjectResponse, MutationError> + Send + Sync;
type TextObjectCreator = dyn Fn(&CreateTextObjectRequest) -> Result<CreateTextObjectResponse, MutationError>
    + Send
    + Sync;
type TextObjectUpdater = dyn Fn(&UpdateTextObjectRequest) -> Result<UpdateTextObjectResponse, TextUpdateError>
    + Send
    + Sync;
type ObjectDuplicator =
    dyn Fn(&DuplicateObjectRequest) -> Result<DuplicateObjectResponse, MutationError> + Send + Sync;
type MediaObjectCreator = dyn Fn(&CreateMediaObjectRequest) -> Result<CreateMediaObjectResponse, MutationError>
    + Send
    + Sync;

struct ServerParts {
    scene_reader: Arc<SceneReader>,
    timeline_reader: Arc<TimelineReader>,
    objects_reader: Arc<ObjectsReader>,
    object_details_reader: Arc<ObjectDetailsReader>,
    object_mover: Arc<ObjectMover>,
    object_deleter: Arc<ObjectDeleter>,
    text_object_creator: Arc<TextObjectCreator>,
    text_object_updater: Arc<TextObjectUpdater>,
    object_duplicator: Arc<ObjectDuplicator>,
    media_object_creator: Arc<MediaObjectCreator>,
}

#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MutationError {
    InvalidPath,
    Unavailable,
    SceneConflict,
    Validation(MoveValidationError),
    ApplyFailed,
    VerifyFailed,
}

#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextUpdateError {
    Unavailable,
    SceneConflict,
    Validation(MoveValidationError),
    NotTextObject,
    TextConflict,
    ApplyFailed,
    VerifyFailed,
}

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
    objects_reader: Arc<ObjectsReader>,
    object_details_reader: Arc<ObjectDetailsReader>,
    object_mover: Arc<ObjectMover>,
    object_deleter: Arc<ObjectDeleter>,
    text_object_creator: Arc<TextObjectCreator>,
    text_object_updater: Arc<TextObjectUpdater>,
    object_duplicator: Arc<ObjectDuplicator>,
    media_object_creator: Arc<MediaObjectCreator>,
    diagnostic_log: Option<HttpDiagnosticLog>,
}

struct HttpDiagnosticLog {
    file: Mutex<File>,
    next_connection_id: AtomicU64,
}

impl HttpDiagnosticLog {
    fn from_env() -> Option<Self> {
        let path = std::env::var_os(HTTP_DIAGNOSTIC_LOG_ENV)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()?;
        Some(Self {
            file: Mutex::new(file),
            next_connection_id: AtomicU64::new(1),
        })
    }

    fn next_connection_id(&self) -> u64 {
        self.next_connection_id.fetch_add(1, Ordering::Relaxed)
    }

    fn event(&self, connection_id: u64, event: &str, fields: serde_json::Value) {
        let timestamp_millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default();
        let record = serde_json::json!({
            "timestampMillis": timestamp_millis,
            "connectionId": connection_id,
            "thread": format!("{:?}", thread::current().id()),
            "event": event,
            "fields": fields,
        });
        if let Ok(mut file) = self.file.lock() {
            let _ = writeln!(file, "{record}");
            let _ = file.flush();
        }
    }
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
            ServerParts {
                scene_reader: platform_scene_reader(),
                timeline_reader: platform_timeline_reader(),
                objects_reader: platform_objects_reader(),
                object_details_reader: platform_object_details_reader(),
                object_mover: platform_object_mover(),
                object_deleter: platform_object_deleter(),
                text_object_creator: platform_text_object_creator(),
                text_object_updater: platform_text_object_updater(),
                object_duplicator: platform_object_duplicator(),
                media_object_creator: platform_media_object_creator(),
            },
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
        parts: ServerParts,
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
            scene_reader: parts.scene_reader,
            timeline_reader: parts.timeline_reader,
            objects_reader: parts.objects_reader,
            object_details_reader: parts.object_details_reader,
            object_mover: parts.object_mover,
            object_deleter: parts.object_deleter,
            text_object_creator: parts.text_object_creator,
            text_object_updater: parts.text_object_updater,
            object_duplicator: parts.object_duplicator,
            media_object_creator: parts.media_object_creator,
            diagnostic_log: HttpDiagnosticLog::from_env(),
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
    let connection_id = context
        .diagnostic_log
        .as_ref()
        .map_or(0, HttpDiagnosticLog::next_connection_id);
    log_http_event(context, connection_id, "accepted", serde_json::json!({}));
    if let Err(error) = configure_accepted_stream(&stream) {
        log_http_io_error(context, connection_id, "configure_stream_failed", &error);
        return;
    }
    if let Err(error) = stream.set_nodelay(true) {
        log_http_io_error(context, connection_id, "set_nodelay_failed", &error);
    }

    let started = Instant::now();
    let Some(request) = read_request(&mut stream, context, connection_id) else {
        return;
    };
    let (method, route_name) = diagnostic_request_label(&request.head);
    log_http_event(
        context,
        connection_id,
        "request_received",
        serde_json::json!({
            "method": method,
            "route": route_name,
            "headBytes": request.head.len(),
            "bodyBytes": request.body.len(),
            "elapsedMillis": started.elapsed().as_millis(),
        }),
    );
    let response = route(&request, context);
    log_http_event(
        context,
        connection_id,
        "route_completed",
        serde_json::json!({
            "status": response.status,
            "bodyBytes": response.body.len(),
            "elapsedMillis": started.elapsed().as_millis(),
        }),
    );
    write_response(&mut stream, &response, context, connection_id);
}

fn configure_accepted_stream(stream: &TcpStream) -> std::io::Result<()> {
    // Windows inherits the listener's nonblocking state on an accepted socket.
    // The listener uses nonblocking polling for shutdown, but each worker handles
    // one complete request synchronously and relies on bounded I/O timeouts.
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))
}

fn write_response(
    stream: &mut TcpStream,
    response: &HttpResponse,
    context: &ServerContext,
    connection_id: u64,
) {
    let mut head = format!(
        "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        response.status,
        response.body.len()
    );
    if let Some(retry_after) = response.retry_after {
        head.push_str(&format!("Retry-After: {retry_after}\r\n"));
    }
    head.push_str("\r\n");
    if let Err(error) = stream.write_all(head.as_bytes()) {
        log_http_io_error(context, connection_id, "write_head_failed", &error);
        return;
    }
    if let Err(error) = stream.write_all(&response.body) {
        log_http_io_error(context, connection_id, "write_body_failed", &error);
        return;
    }
    if let Err(error) = stream.flush() {
        log_http_io_error(context, connection_id, "flush_failed", &error);
        return;
    }
    log_http_event(
        context,
        connection_id,
        "response_flushed",
        serde_json::json!({
            "headBytes": head.len(),
            "bodyBytes": response.body.len(),
        }),
    );
}

struct HttpRequest {
    head: String,
    body: Vec<u8>,
}

fn read_request(
    stream: &mut TcpStream,
    context: &ServerContext,
    connection_id: u64,
) -> Option<HttpRequest> {
    let mut request = Vec::with_capacity(512);
    let mut chunk = [0_u8; 512];
    while request.len() < MAX_REQUEST_HEAD {
        match stream.read(&mut chunk) {
            Ok(0) => {
                log_http_event(
                    context,
                    connection_id,
                    "read_head_eof",
                    serde_json::json!({"bytesRead": request.len()}),
                );
                return None;
            }
            Err(error) => {
                log_http_io_error(context, connection_id, "read_head_failed", &error);
                return None;
            }
            Ok(count) => {
                request.extend_from_slice(&chunk[..count]);
                if let Some(head_end) = request
                    .windows(4)
                    .position(|window| window == b"\r\n\r\n")
                    .map(|position| position + 4)
                {
                    let head = std::str::from_utf8(&request[..head_end]).ok()?.to_owned();
                    let content_length = parse_content_length(&head)?;
                    if content_length > MAX_REQUEST_BODY {
                        let response = api_error(
                            "413 Payload Too Large",
                            ErrorCode::InvalidRequest,
                            "Request body exceeds the 16 KiB limit",
                            false,
                            None,
                        );
                        write_response(stream, &response, context, connection_id);
                        // Keep the socket alive briefly so Winsock delivers the rejection
                        // before closing a connection whose advertised body remains unread.
                        thread::sleep(IO_TIMEOUT);
                        return None;
                    }
                    let mut body = request[head_end..].to_vec();
                    let body_deadline = Instant::now() + EXPECT_BODY_TIMEOUT;
                    if expects_continue(&head) && body.len() < content_length {
                        if let Err(error) = stream.set_read_timeout(Some(EXPECT_BODY_TIMEOUT)) {
                            log_http_io_error(
                                context,
                                connection_id,
                                "set_expect_timeout_failed",
                                &error,
                            );
                            return None;
                        }
                        if let Err(error) = stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n") {
                            log_http_io_error(
                                context,
                                connection_id,
                                "write_continue_failed",
                                &error,
                            );
                            return None;
                        }
                        if let Err(error) = stream.flush() {
                            log_http_io_error(
                                context,
                                connection_id,
                                "flush_continue_failed",
                                &error,
                            );
                            return None;
                        }
                    }
                    while body.len() < content_length {
                        match stream.read(&mut chunk) {
                            Ok(0) => {
                                log_http_event(
                                    context,
                                    connection_id,
                                    "read_body_eof",
                                    serde_json::json!({
                                        "bytesRead": body.len(),
                                        "expectedBytes": content_length,
                                    }),
                                );
                                return None;
                            }
                            Ok(count) => body.extend_from_slice(&chunk[..count]),
                            Err(error)
                                if matches!(
                                    error.kind(),
                                    std::io::ErrorKind::Interrupted
                                        | std::io::ErrorKind::WouldBlock
                                        | std::io::ErrorKind::TimedOut
                                ) && Instant::now() < body_deadline =>
                            {
                                thread::sleep(Duration::from_millis(1));
                                continue;
                            }
                            Err(error) => {
                                log_http_io_error(
                                    context,
                                    connection_id,
                                    "read_body_failed",
                                    &error,
                                );
                                return None;
                            }
                        }
                        if body.len() > content_length {
                            return None;
                        }
                    }
                    return Some(HttpRequest { head, body });
                }
            }
        }
    }
    None
}

fn log_http_event(
    context: &ServerContext,
    connection_id: u64,
    event: &str,
    fields: serde_json::Value,
) {
    if let Some(log) = &context.diagnostic_log {
        log.event(connection_id, event, fields);
    }
}

fn log_http_io_error(
    context: &ServerContext,
    connection_id: u64,
    event: &str,
    error: &std::io::Error,
) {
    log_http_event(
        context,
        connection_id,
        event,
        serde_json::json!({
            "errorKind": format!("{:?}", error.kind()),
            "rawOsError": error.raw_os_error(),
            "message": error.to_string(),
        }),
    );
}

fn diagnostic_request_label(head: &str) -> (&str, &'static str) {
    let mut parts = head
        .split("\r\n")
        .next()
        .unwrap_or_default()
        .split_whitespace();
    let method = parts.next().unwrap_or("unknown");
    let route = match parts.next() {
        Some("/healthz") => "health",
        Some("/v1/status") => "status",
        Some("/v1/scenes/current") => "current_scene",
        Some("/v1/scenes/current/timeline") => "current_timeline",
        Some("/v1/scenes/current/objects") => "current_objects",
        Some("/v1/scenes/current/objects/details") => "current_object_details",
        Some("/v1/scenes/current/objects/move") => "move_object",
        Some("/v1/scenes/current/objects/delete") => "delete_object",
        Some("/v1/scenes/current/objects/text") => "create_text_object",
        Some("/v1/scenes/current/objects/text/update") => "update_text_object",
        Some("/v1/scenes/current/objects/duplicate") => "duplicate_object",
        Some("/v1/scenes/current/objects/media") => "create_media_object",
        _ => "other",
    };
    (method, route)
}

fn expects_continue(head: &str) -> bool {
    head.split("\r\n")
        .skip(1)
        .take_while(|line| !line.is_empty())
        .filter_map(|line| line.split_once(':'))
        .any(|(name, value)| {
            name.eq_ignore_ascii_case("expect") && value.trim().eq_ignore_ascii_case("100-continue")
        })
}

fn parse_content_length(head: &str) -> Option<usize> {
    let mut content_length = None;
    for line in head
        .split("\r\n")
        .skip(1)
        .take_while(|line| !line.is_empty())
    {
        let Some((name, value)) = line.split_once(':') else {
            return Some(0);
        };
        if name.eq_ignore_ascii_case("content-length") {
            if content_length.is_some() {
                return None;
            }
            content_length = Some(value.trim().parse().ok()?);
        }
    }
    Some(content_length.unwrap_or(0))
}

struct HttpResponse {
    status: &'static str,
    body: Vec<u8>,
    retry_after: Option<u64>,
}

fn route(request: &HttpRequest, context: &ServerContext) -> HttpResponse {
    let mut lines = request.head.split("\r\n");
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
    if parts.next().is_some() || version != Some("HTTP/1.1") {
        return api_error(
            "400 Bad Request",
            ErrorCode::InvalidRequest,
            "Invalid request line",
            false,
            None,
        );
    }

    match (method, path) {
        (Some("GET"), Some("/healthz")) => json_response(&Health {
            status: HealthStatus::Ok,
            plugin_version: env!("CARGO_PKG_VERSION").to_owned(),
        }),
        (Some("GET"), Some("/v1/status")) => json_response(&context.status),
        (Some("GET"), Some("/v1/scenes/current")) => {
            match context.editor_gate.read(|| (context.scene_reader)()) {
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
            }
        }
        (Some("GET"), Some("/v1/scenes/current/timeline")) => {
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
        (Some("GET"), Some("/v1/scenes/current/objects")) => {
            match context.editor_gate.read(|| (context.objects_reader)()) {
                Ok(objects) => json_response(&objects),
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
        (Some("GET"), Some("/v1/scenes/current/objects/details")) => {
            match context
                .editor_gate
                .read(|| (context.object_details_reader)())
            {
                Ok(objects) => json_response(&objects),
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
        (Some("POST"), Some("/v1/scenes/current/objects/move")) => {
            if !has_json_content_type(&request.head) {
                api_error(
                    "400 Bad Request",
                    ErrorCode::InvalidRequest,
                    "Content-Type must be application/json",
                    false,
                    None,
                )
            } else {
                move_object(&request.body, context)
            }
        }
        (Some("POST"), Some("/v1/scenes/current/objects/delete")) => {
            if !has_json_content_type(&request.head) {
                api_error(
                    "400 Bad Request",
                    ErrorCode::InvalidRequest,
                    "Content-Type must be application/json",
                    false,
                    None,
                )
            } else {
                delete_object(&request.body, context)
            }
        }
        (Some("POST"), Some("/v1/scenes/current/objects/text")) => {
            if !has_json_content_type(&request.head) {
                api_error(
                    "400 Bad Request",
                    ErrorCode::InvalidRequest,
                    "Content-Type must be application/json",
                    false,
                    None,
                )
            } else {
                create_text_object(&request.body, context)
            }
        }
        (Some("POST"), Some("/v1/scenes/current/objects/text/update")) => {
            if !has_json_content_type(&request.head) {
                api_error(
                    "400 Bad Request",
                    ErrorCode::InvalidRequest,
                    "Content-Type must be application/json",
                    false,
                    None,
                )
            } else {
                update_text_object(&request.body, context)
            }
        }
        (Some("POST"), Some("/v1/scenes/current/objects/duplicate")) => {
            if !has_json_content_type(&request.head) {
                api_error(
                    "400 Bad Request",
                    ErrorCode::InvalidRequest,
                    "Content-Type must be application/json",
                    false,
                    None,
                )
            } else {
                duplicate_object(&request.body, context)
            }
        }
        (Some("POST"), Some("/v1/scenes/current/objects/media")) => {
            if !has_json_content_type(&request.head) {
                api_error(
                    "400 Bad Request",
                    ErrorCode::InvalidRequest,
                    "Content-Type must be application/json",
                    false,
                    None,
                )
            } else {
                create_media_object(&request.body, context)
            }
        }
        (Some("GET"), _) => api_error(
            "404 Not Found",
            ErrorCode::RouteNotFound,
            "Route not found",
            false,
            None,
        ),
        _ => api_error(
            "400 Bad Request",
            ErrorCode::InvalidRequest,
            "Invalid request method",
            false,
            None,
        ),
    }
}

fn has_json_content_type(head: &str) -> bool {
    head.split("\r\n")
        .skip(1)
        .take_while(|line| !line.is_empty())
        .filter_map(|line| line.split_once(':'))
        .any(|(name, value)| {
            name.eq_ignore_ascii_case("content-type")
                && value
                    .trim()
                    .split(';')
                    .next()
                    .is_some_and(|media_type| media_type.eq_ignore_ascii_case("application/json"))
        })
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

fn move_object(body: &[u8], context: &ServerContext) -> HttpResponse {
    let request = match serde_json::from_slice::<MoveObjectRequest>(body) {
        Ok(request) => request,
        Err(_) => {
            return api_error(
                "400 Bad Request",
                ErrorCode::InvalidRequest,
                "Invalid move request",
                false,
                None,
            );
        }
    };
    match context
        .editor_gate
        .read(|| Ok((context.object_mover)(&request)))
    {
        Ok(Ok(response)) => json_response(&response),
        Ok(Err(MutationError::SceneConflict)) => api_error(
            "409 Conflict",
            ErrorCode::StateConflict,
            "Current scene does not match the request",
            false,
            None,
        ),
        Ok(Err(MutationError::Validation(MoveValidationError::TargetNotFound))) => api_error(
            "404 Not Found",
            ErrorCode::ObjectNotFound,
            "Target object was not found",
            false,
            None,
        ),
        Ok(Err(MutationError::Validation(_))) => api_error(
            "409 Conflict",
            ErrorCode::StateConflict,
            "Object snapshot or destination conflicts with current state",
            false,
            None,
        ),
        Ok(Err(MutationError::Unavailable)) => api_error(
            "503 Service Unavailable",
            ErrorCode::EditorUnavailable,
            "AviUtl2 did not accept the edit",
            true,
            Some(RETRY_AFTER_SECONDS),
        ),
        Ok(Err(MutationError::ApplyFailed | MutationError::VerifyFailed)) => api_error(
            "500 Internal Server Error",
            ErrorCode::MutationOutcomeUnknown,
            "Move outcome is unknown; re-read current objects before continuing",
            false,
            None,
        ),
        Ok(Err(MutationError::InvalidPath)) => internal_mutation_error(),
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
            "AviUtl2 did not accept the edit",
            true,
            Some(RETRY_AFTER_SECONDS),
        ),
    }
}

fn delete_object(body: &[u8], context: &ServerContext) -> HttpResponse {
    let request = match serde_json::from_slice::<DeleteObjectRequest>(body) {
        Ok(request) => request,
        Err(_) => {
            return api_error(
                "400 Bad Request",
                ErrorCode::InvalidRequest,
                "Invalid delete request",
                false,
                None,
            );
        }
    };
    match context
        .editor_gate
        .read(|| Ok((context.object_deleter)(&request)))
    {
        Ok(Ok(response)) => json_response(&response),
        Ok(Err(MutationError::SceneConflict)) => api_error(
            "409 Conflict",
            ErrorCode::StateConflict,
            "Current scene does not match the request",
            false,
            None,
        ),
        Ok(Err(MutationError::Validation(MoveValidationError::TargetNotFound))) => api_error(
            "404 Not Found",
            ErrorCode::ObjectNotFound,
            "Target object was not found",
            false,
            None,
        ),
        Ok(Err(MutationError::Validation(_))) => api_error(
            "409 Conflict",
            ErrorCode::StateConflict,
            "Object snapshot conflicts with current state",
            false,
            None,
        ),
        Ok(Err(MutationError::Unavailable)) => api_error(
            "503 Service Unavailable",
            ErrorCode::EditorUnavailable,
            "AviUtl2 did not accept the edit",
            true,
            Some(RETRY_AFTER_SECONDS),
        ),
        Ok(Err(MutationError::ApplyFailed | MutationError::VerifyFailed)) => api_error(
            "500 Internal Server Error",
            ErrorCode::MutationOutcomeUnknown,
            "Delete outcome is unknown; re-read current objects before continuing",
            false,
            None,
        ),
        Ok(Err(MutationError::InvalidPath)) => internal_mutation_error(),
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
            "AviUtl2 did not accept the edit",
            true,
            Some(RETRY_AFTER_SECONDS),
        ),
    }
}

fn create_text_object(body: &[u8], context: &ServerContext) -> HttpResponse {
    let request = match serde_json::from_slice::<CreateTextObjectRequest>(body) {
        Ok(request)
            if request.length > 0
                && !request
                    .text
                    .chars()
                    .any(|character| matches!(character, '\r' | '\n' | '\0')) =>
        {
            request
        }
        _ => {
            return api_error(
                "400 Bad Request",
                ErrorCode::InvalidRequest,
                "Invalid text object request",
                false,
                None,
            );
        }
    };
    match context
        .editor_gate
        .read(|| Ok((context.text_object_creator)(&request)))
    {
        Ok(Ok(response)) => json_response(&response),
        Ok(Err(MutationError::SceneConflict)) => api_error(
            "409 Conflict",
            ErrorCode::StateConflict,
            "Current scene does not match the request",
            false,
            None,
        ),
        Ok(Err(MutationError::Validation(_))) => api_error(
            "409 Conflict",
            ErrorCode::StateConflict,
            "Text object destination conflicts with current state",
            false,
            None,
        ),
        Ok(Err(MutationError::Unavailable)) => api_error(
            "503 Service Unavailable",
            ErrorCode::EditorUnavailable,
            "AviUtl2 did not accept the edit",
            true,
            Some(RETRY_AFTER_SECONDS),
        ),
        Ok(Err(MutationError::ApplyFailed | MutationError::VerifyFailed)) => api_error(
            "500 Internal Server Error",
            ErrorCode::MutationOutcomeUnknown,
            "Text object outcome is unknown; re-read current objects before continuing",
            false,
            None,
        ),
        Ok(Err(MutationError::InvalidPath)) => internal_mutation_error(),
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
            "AviUtl2 did not accept the edit",
            true,
            Some(RETRY_AFTER_SECONDS),
        ),
    }
}

fn update_text_object(body: &[u8], context: &ServerContext) -> HttpResponse {
    let request = match serde_json::from_slice::<UpdateTextObjectRequest>(body) {
        Ok(request) => request,
        Err(_) => {
            return api_error(
                "400 Bad Request",
                ErrorCode::InvalidRequest,
                "Invalid text update JSON",
                false,
                None,
            );
        }
    };
    if request
        .text
        .chars()
        .any(|character| matches!(character, '\r' | '\n' | '\0'))
    {
        return api_error(
            "400 Bad Request",
            ErrorCode::InvalidRequest,
            "text must not contain CR, LF, or NUL",
            false,
            None,
        );
    }
    match context
        .editor_gate
        .read(|| Ok((context.text_object_updater)(&request)))
    {
        Ok(Ok(response)) => json_response(&response),
        Ok(Err(TextUpdateError::SceneConflict | TextUpdateError::TextConflict)) => api_error(
            "409 Conflict",
            ErrorCode::StateConflict,
            "Current scene or text does not match the request",
            false,
            None,
        ),
        Ok(Err(TextUpdateError::Validation(MoveValidationError::TargetNotFound))) => api_error(
            "404 Not Found",
            ErrorCode::ObjectNotFound,
            "Target object was not found",
            false,
            None,
        ),
        Ok(Err(TextUpdateError::Validation(_) | TextUpdateError::NotTextObject)) => api_error(
            "409 Conflict",
            ErrorCode::StateConflict,
            "Object snapshot does not identify a text object",
            false,
            None,
        ),
        Ok(Err(TextUpdateError::Unavailable)) => api_error(
            "503 Service Unavailable",
            ErrorCode::EditorUnavailable,
            "AviUtl2 did not accept the edit",
            true,
            Some(RETRY_AFTER_SECONDS),
        ),
        Ok(Err(TextUpdateError::ApplyFailed | TextUpdateError::VerifyFailed)) => api_error(
            "500 Internal Server Error",
            ErrorCode::MutationOutcomeUnknown,
            "Text update outcome is unknown; re-read current object details before continuing",
            false,
            None,
        ),
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
            "AviUtl2 did not accept the edit",
            true,
            Some(RETRY_AFTER_SECONDS),
        ),
    }
}

fn duplicate_object(body: &[u8], context: &ServerContext) -> HttpResponse {
    let request = match serde_json::from_slice::<DuplicateObjectRequest>(body) {
        Ok(request) => request,
        Err(_) => {
            return api_error(
                "400 Bad Request",
                ErrorCode::InvalidRequest,
                "Invalid duplicate request",
                false,
                None,
            );
        }
    };
    match context
        .editor_gate
        .read(|| Ok((context.object_duplicator)(&request)))
    {
        Ok(Ok(response)) => json_response(&response),
        Ok(Err(MutationError::SceneConflict)) => api_error(
            "409 Conflict",
            ErrorCode::StateConflict,
            "Current scene does not match the request",
            false,
            None,
        ),
        Ok(Err(MutationError::Validation(MoveValidationError::TargetNotFound))) => api_error(
            "404 Not Found",
            ErrorCode::ObjectNotFound,
            "Target object was not found",
            false,
            None,
        ),
        Ok(Err(MutationError::Validation(_))) => api_error(
            "409 Conflict",
            ErrorCode::StateConflict,
            "Object snapshot or destination conflicts with current state",
            false,
            None,
        ),
        Ok(Err(MutationError::Unavailable)) => api_error(
            "503 Service Unavailable",
            ErrorCode::EditorUnavailable,
            "AviUtl2 did not accept the edit",
            true,
            Some(RETRY_AFTER_SECONDS),
        ),
        Ok(Err(MutationError::ApplyFailed | MutationError::VerifyFailed)) => api_error(
            "500 Internal Server Error",
            ErrorCode::MutationOutcomeUnknown,
            "Duplicate outcome is unknown; re-read current objects before continuing",
            false,
            None,
        ),
        Ok(Err(MutationError::InvalidPath)) => internal_mutation_error(),
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
            "AviUtl2 did not accept the edit",
            true,
            Some(RETRY_AFTER_SECONDS),
        ),
    }
}

fn create_media_object(body: &[u8], context: &ServerContext) -> HttpResponse {
    let request = match serde_json::from_slice::<CreateMediaObjectRequest>(body) {
        Ok(request) if request.length > 0 => request,
        _ => {
            return api_error(
                "400 Bad Request",
                ErrorCode::InvalidRequest,
                "Invalid media object request",
                false,
                None,
            );
        }
    };
    match context
        .editor_gate
        .read(|| Ok((context.media_object_creator)(&request)))
    {
        Ok(Ok(response)) => json_response(&response),
        Ok(Err(MutationError::InvalidPath)) => api_error(
            "400 Bad Request",
            ErrorCode::InvalidRequest,
            "mediaPath must be an absolute path to an existing file",
            false,
            None,
        ),
        Ok(Err(MutationError::SceneConflict | MutationError::Validation(_))) => api_error(
            "409 Conflict",
            ErrorCode::StateConflict,
            "Media object destination conflicts with current state",
            false,
            None,
        ),
        Ok(Err(MutationError::Unavailable)) => api_error(
            "503 Service Unavailable",
            ErrorCode::EditorUnavailable,
            "AviUtl2 did not accept the edit",
            true,
            Some(RETRY_AFTER_SECONDS),
        ),
        Ok(Err(MutationError::ApplyFailed | MutationError::VerifyFailed)) => api_error(
            "500 Internal Server Error",
            ErrorCode::MutationOutcomeUnknown,
            "Media object outcome is unknown; re-read current objects before continuing",
            false,
            None,
        ),
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
            "AviUtl2 did not accept the edit",
            true,
            Some(RETRY_AFTER_SECONDS),
        ),
    }
}

fn internal_mutation_error() -> HttpResponse {
    api_error(
        "500 Internal Server Error",
        ErrorCode::InternalError,
        "Plugin internal error",
        false,
        None,
    )
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
            width: sdk_u64(info.width),
            height: sdk_u64(info.height),
            frame_rate: FrameRate {
                numerator: *info.fps.numer(),
                denominator: *info.fps.denom(),
            },
            cursor_frame: sdk_u64(info.frame),
            object_end_frame: sdk_u64(info.frame_max),
            highest_object_layer: sdk_u64(info.layer_max),
        })
    })
}

#[cfg(windows)]
fn sdk_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(windows)]
fn sdk_usize(value: u64) -> Result<usize, MutationError> {
    usize::try_from(value)
        .map_err(|_| MutationError::Validation(MoveValidationError::FrameOverflow))
}

#[cfg(windows)]
fn platform_objects_reader() -> Arc<ObjectsReader> {
    Arc::new(|| {
        if !crate::windows_plugin::EDIT_HANDLE.is_ready() {
            return Err(EditorError::Unavailable);
        }
        let info = crate::windows_plugin::EDIT_HANDLE.get_edit_info();
        crate::windows_plugin::EDIT_HANDLE
            .call_read_section(|section| {
                let mut objects = Vec::new();
                for layer in 0..=info.layer_max {
                    for (position, handle) in section.objects_in_layer(layer) {
                        objects.push(TimelineObject {
                            layer: sdk_u64(position.layer),
                            start_frame: sdk_u64(position.start),
                            end_frame: sdk_u64(position.end),
                            name: section.get_object_name(handle).ok().flatten(),
                        });
                    }
                }
                CurrentObjects { objects }
            })
            .map_err(|_| EditorError::Unavailable)
    })
}

#[cfg(windows)]
fn platform_object_details_reader() -> Arc<ObjectDetailsReader> {
    Arc::new(|| {
        if !crate::windows_plugin::EDIT_HANDLE.is_ready() {
            return Err(EditorError::Unavailable);
        }
        let info = crate::windows_plugin::EDIT_HANDLE.get_edit_info();
        crate::windows_plugin::EDIT_HANDLE
            .call_read_section(|section| {
                let mut objects = Vec::new();
                for layer in 0..=info.layer_max {
                    for (position, handle) in section.objects_in_layer(layer) {
                        let object = TimelineObject {
                            layer: sdk_u64(position.layer),
                            start_frame: sdk_u64(position.start),
                            end_frame: sdk_u64(position.end),
                            name: section.get_object_name(handle).ok().flatten(),
                        };
                        let primary_effect = section
                            .get_first_effect(handle)
                            .and_then(|effect| section.get_effect_name(effect))
                            .ok();
                        let (kind, text) = match primary_effect.as_deref() {
                            Some(TEXT_EFFECT_NAME) => (
                                ObjectKind::Text,
                                section
                                    .get_object_effect_item(
                                        handle,
                                        TEXT_EFFECT_NAME,
                                        0,
                                        TEXT_ITEM_NAME,
                                    )
                                    .ok(),
                            ),
                            Some(IMAGE_EFFECT_NAME) => (ObjectKind::Image, None),
                            Some(AUDIO_EFFECT_NAME) => (ObjectKind::Audio, None),
                            _ => (ObjectKind::Unknown, None),
                        };
                        objects.push(ObjectDetails { object, kind, text });
                    }
                }
                Ok(CurrentObjectDetails { objects })
            })
            .map_err(|_| EditorError::Unavailable)?
    })
}

#[cfg(windows)]
fn platform_object_mover() -> Arc<ObjectMover> {
    Arc::new(|request| {
        let destination_layer = sdk_usize(request.destination.layer)?;
        let destination_start_frame = sdk_usize(request.destination.start_frame)?;
        if !crate::windows_plugin::EDIT_HANDLE.is_ready() {
            return Err(MutationError::Unavailable);
        }
        let layer_max = crate::windows_plugin::EDIT_HANDLE.get_edit_info().layer_max;
        crate::windows_plugin::EDIT_HANDLE
            .call_edit_section(|section| {
                let scene_name = section
                    .get_scene_name()
                    .map_err(|_| MutationError::Unavailable)?;
                if scene_name != request.expected_scene_name {
                    return Err(MutationError::SceneConflict);
                }

                let mut handles = Vec::new();
                let mut objects = Vec::new();
                for layer in 0..=layer_max {
                    for (position, handle) in section.objects_in_layer(layer) {
                        handles.push(handle);
                        objects.push(TimelineObject {
                            layer: sdk_u64(position.layer),
                            start_frame: sdk_u64(position.start),
                            end_frame: sdk_u64(position.end),
                            name: section.get_object_name(handle).ok().flatten(),
                        });
                    }
                }
                let (target_index, expected) =
                    crate::mutation::validate_move(&objects, &request.target, &request.destination)
                        .map_err(MutationError::Validation)?;
                let handle = handles[target_index];
                section
                    .move_object(handle, destination_layer, destination_start_frame)
                    .map_err(|_| MutationError::ApplyFailed)?;
                let position = section
                    .get_object_layer_frame(handle)
                    .map_err(|_| MutationError::VerifyFailed)?;
                let actual = TimelineObject {
                    layer: sdk_u64(position.layer),
                    start_frame: sdk_u64(position.start),
                    end_frame: sdk_u64(position.end),
                    name: section
                        .get_object_name(handle)
                        .map_err(|_| MutationError::VerifyFailed)?,
                };
                if actual != expected {
                    return Err(MutationError::VerifyFailed);
                }
                Ok(MoveObjectResponse { object: actual })
            })
            .map_err(|_| MutationError::Unavailable)?
    })
}

#[cfg(windows)]
fn alias_without_frame(alias: &str) -> String {
    alias
        .lines()
        .filter(|line| !line.starts_with("frame="))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(windows)]
fn platform_object_deleter() -> Arc<ObjectDeleter> {
    Arc::new(|request| {
        if !crate::windows_plugin::EDIT_HANDLE.is_ready() {
            return Err(MutationError::Unavailable);
        }
        let layer_max = crate::windows_plugin::EDIT_HANDLE.get_edit_info().layer_max;
        crate::windows_plugin::EDIT_HANDLE
            .call_edit_section(|section| {
                let scene_name = section
                    .get_scene_name()
                    .map_err(|_| MutationError::Unavailable)?;
                if scene_name != request.expected_scene_name {
                    return Err(MutationError::SceneConflict);
                }

                let mut handles = Vec::new();
                let mut objects = Vec::new();
                for layer in 0..=layer_max {
                    for (position, handle) in section.objects_in_layer(layer) {
                        handles.push(handle);
                        objects.push(TimelineObject {
                            layer: sdk_u64(position.layer),
                            start_frame: sdk_u64(position.start),
                            end_frame: sdk_u64(position.end),
                            name: section.get_object_name(handle).ok().flatten(),
                        });
                    }
                }
                let target_index = crate::mutation::locate_exact(&objects, &request.target)
                    .map_err(MutationError::Validation)?;
                let handle = handles[target_index];
                section
                    .delete_object(handle)
                    .map_err(|_| MutationError::ApplyFailed)?;
                if section.object_exists(handle) {
                    return Err(MutationError::VerifyFailed);
                }
                Ok(DeleteObjectResponse {
                    deleted: request.target.clone(),
                })
            })
            .map_err(|_| MutationError::Unavailable)?
    })
}

#[cfg(windows)]
fn platform_text_object_creator() -> Arc<TextObjectCreator> {
    Arc::new(|request| {
        let layer = sdk_usize(request.layer)?;
        let start_frame = sdk_usize(request.start_frame)?;
        let length = sdk_usize(request.length)?;
        if !crate::windows_plugin::EDIT_HANDLE.is_ready() {
            return Err(MutationError::Unavailable);
        }
        let layer_max = crate::windows_plugin::EDIT_HANDLE.get_edit_info().layer_max;
        crate::windows_plugin::EDIT_HANDLE
            .call_edit_section(|section| {
                let scene_name = section
                    .get_scene_name()
                    .map_err(|_| MutationError::Unavailable)?;
                if scene_name != request.expected_scene_name {
                    return Err(MutationError::SceneConflict);
                }
                let mut objects = Vec::new();
                for layer in 0..=layer_max {
                    for (position, handle) in section.objects_in_layer(layer) {
                        objects.push(TimelineObject {
                            layer: sdk_u64(position.layer),
                            start_frame: sdk_u64(position.start),
                            end_frame: sdk_u64(position.end),
                            name: section.get_object_name(handle).ok().flatten(),
                        });
                    }
                }
                let expected = crate::mutation::validate_create(
                    &objects,
                    request.layer,
                    request.start_frame,
                    request.length,
                )
                .map_err(MutationError::Validation)?;
                let alias = format!(
                    "[Object]\r\n[Object.0]\r\neffect.name=テキスト\r\nテキスト={}\r\n[Object.1]\r\neffect.name=標準描画\r\n",
                    request.text
                );
                let handle = section
                    .create_object_from_alias(&alias, layer, start_frame, length)
                    .map_err(|_| MutationError::ApplyFailed)?;
                let position = section
                    .get_object_layer_frame(handle)
                    .map_err(|_| MutationError::VerifyFailed)?;
                let actual = TimelineObject {
                    layer: sdk_u64(position.layer),
                    start_frame: sdk_u64(position.start),
                    end_frame: sdk_u64(position.end),
                    name: section
                        .get_object_name(handle)
                        .map_err(|_| MutationError::VerifyFailed)?,
                };
                let text = section
                    .get_object_effect_item(handle, TEXT_EFFECT_NAME, 0, TEXT_ITEM_NAME)
                    .map_err(|_| MutationError::VerifyFailed)?;
                if actual != expected || text != request.text {
                    return Err(MutationError::VerifyFailed);
                }
                Ok(CreateTextObjectResponse {
                    object: actual,
                    text,
                })
            })
            .map_err(|_| MutationError::Unavailable)?
    })
}

#[cfg(windows)]
fn platform_text_object_updater() -> Arc<TextObjectUpdater> {
    Arc::new(|request| {
        if !crate::windows_plugin::EDIT_HANDLE.is_ready() {
            return Err(TextUpdateError::Unavailable);
        }
        let layer_max = crate::windows_plugin::EDIT_HANDLE.get_edit_info().layer_max;
        crate::windows_plugin::EDIT_HANDLE
            .call_edit_section(|section| {
                let scene_name = section
                    .get_scene_name()
                    .map_err(|_| TextUpdateError::Unavailable)?;
                if scene_name != request.expected_scene_name {
                    return Err(TextUpdateError::SceneConflict);
                }
                let mut handles = Vec::new();
                let mut objects = Vec::new();
                for layer in 0..=layer_max {
                    for (position, handle) in section.objects_in_layer(layer) {
                        handles.push(handle);
                        objects.push(TimelineObject {
                            layer: sdk_u64(position.layer),
                            start_frame: sdk_u64(position.start),
                            end_frame: sdk_u64(position.end),
                            name: section.get_object_name(handle).ok().flatten(),
                        });
                    }
                }
                let target_index = crate::mutation::locate_exact(&objects, &request.target)
                    .map_err(TextUpdateError::Validation)?;
                let handle = handles[target_index];
                let primary_effect = section
                    .get_first_effect(handle)
                    .and_then(|effect| section.get_effect_name(effect))
                    .map_err(|_| TextUpdateError::Unavailable)?;
                if primary_effect != TEXT_EFFECT_NAME {
                    return Err(TextUpdateError::NotTextObject);
                }
                let current_text = section
                    .get_object_effect_item(handle, TEXT_EFFECT_NAME, 0, TEXT_ITEM_NAME)
                    .map_err(|_| TextUpdateError::Unavailable)?;
                if current_text != request.expected_text {
                    return Err(TextUpdateError::TextConflict);
                }
                section
                    .set_object_effect_item(
                        handle,
                        TEXT_EFFECT_NAME,
                        0,
                        TEXT_ITEM_NAME,
                        &request.text,
                    )
                    .map_err(|_| TextUpdateError::ApplyFailed)?;
                let text = section
                    .get_object_effect_item(handle, TEXT_EFFECT_NAME, 0, TEXT_ITEM_NAME)
                    .map_err(|_| TextUpdateError::VerifyFailed)?;
                let position = section
                    .get_object_layer_frame(handle)
                    .map_err(|_| TextUpdateError::VerifyFailed)?;
                let object = TimelineObject {
                    layer: sdk_u64(position.layer),
                    start_frame: sdk_u64(position.start),
                    end_frame: sdk_u64(position.end),
                    name: section
                        .get_object_name(handle)
                        .map_err(|_| TextUpdateError::VerifyFailed)?,
                };
                if text != request.text || object != request.target {
                    return Err(TextUpdateError::VerifyFailed);
                }
                Ok(UpdateTextObjectResponse { object, text })
            })
            .map_err(|_| TextUpdateError::Unavailable)?
    })
}

#[cfg(windows)]
fn platform_object_duplicator() -> Arc<ObjectDuplicator> {
    Arc::new(|request| {
        if !crate::windows_plugin::EDIT_HANDLE.is_ready() {
            return Err(MutationError::Unavailable);
        }
        let layer_max = crate::windows_plugin::EDIT_HANDLE.get_edit_info().layer_max;
        crate::windows_plugin::EDIT_HANDLE
            .call_edit_section(|section| {
                let scene_name = section
                    .get_scene_name()
                    .map_err(|_| MutationError::Unavailable)?;
                if scene_name != request.expected_scene_name {
                    return Err(MutationError::SceneConflict);
                }
                let mut handles = Vec::new();
                let mut objects = Vec::new();
                for layer in 0..=layer_max {
                    for (position, handle) in section.objects_in_layer(layer) {
                        handles.push(handle);
                        objects.push(TimelineObject {
                            layer: sdk_u64(position.layer),
                            start_frame: sdk_u64(position.start),
                            end_frame: sdk_u64(position.end),
                            name: section.get_object_name(handle).ok().flatten(),
                        });
                    }
                }
                let (target_index, expected) = crate::mutation::validate_duplicate(
                    &objects,
                    &request.target,
                    &request.destination,
                )
                .map_err(MutationError::Validation)?;
                let alias = section
                    .get_object_alias(handles[target_index])
                    .map_err(|_| MutationError::ApplyFailed)?;
                let length = expected.end_frame - expected.start_frame + 1;
                let layer = sdk_usize(expected.layer)?;
                let start_frame = sdk_usize(expected.start_frame)?;
                let length = sdk_usize(length)?;
                let handle = section
                    .create_object_from_alias(&alias, layer, start_frame, length)
                    .map_err(|_| MutationError::ApplyFailed)?;
                let position = section
                    .get_object_layer_frame(handle)
                    .map_err(|_| MutationError::VerifyFailed)?;
                let actual = TimelineObject {
                    layer: sdk_u64(position.layer),
                    start_frame: sdk_u64(position.start),
                    end_frame: sdk_u64(position.end),
                    name: section
                        .get_object_name(handle)
                        .map_err(|_| MutationError::VerifyFailed)?,
                };
                let duplicate_alias = section
                    .get_object_alias(handle)
                    .map_err(|_| MutationError::VerifyFailed)?;
                if actual != expected
                    || alias_without_frame(&duplicate_alias) != alias_without_frame(&alias)
                {
                    return Err(MutationError::VerifyFailed);
                }
                Ok(DuplicateObjectResponse { object: actual })
            })
            .map_err(|_| MutationError::Unavailable)?
    })
}

#[cfg(windows)]
fn platform_media_object_creator() -> Arc<MediaObjectCreator> {
    Arc::new(|request| {
        let media_path = std::path::Path::new(&request.media_path);
        if !media_path.is_absolute() || !media_path.is_file() {
            return Err(MutationError::InvalidPath);
        }
        let layer = sdk_usize(request.layer)?;
        let start_frame = sdk_usize(request.start_frame)?;
        let length = sdk_usize(request.length)?;
        if !crate::windows_plugin::EDIT_HANDLE.is_ready() {
            return Err(MutationError::Unavailable);
        }
        let layer_max = crate::windows_plugin::EDIT_HANDLE.get_edit_info().layer_max;
        let result = crate::windows_plugin::EDIT_HANDLE
            .call_edit_section(|section| {
                let scene_name = section
                    .get_scene_name()
                    .map_err(|_| MutationError::Unavailable)?;
                if scene_name != request.expected_scene_name {
                    return Err(MutationError::SceneConflict);
                }
                let mut objects = Vec::new();
                for layer in 0..=layer_max {
                    for (position, handle) in section.objects_in_layer(layer) {
                        objects.push(TimelineObject {
                            layer: sdk_u64(position.layer),
                            start_frame: sdk_u64(position.start),
                            end_frame: sdk_u64(position.end),
                            name: section.get_object_name(handle).ok().flatten(),
                        });
                    }
                }
                let expected = crate::mutation::validate_create(
                    &objects,
                    request.layer,
                    request.start_frame,
                    request.length,
                )
                .map_err(MutationError::Validation)?;
                let handle = section
                    .create_object_from_media_file(media_path, layer, start_frame, Some(length))
                    .map_err(|_| MutationError::ApplyFailed)?;
                let position = section
                    .get_object_layer_frame(handle)
                    .map_err(|_| MutationError::VerifyFailed)?;
                if sdk_u64(position.layer) != expected.layer
                    || sdk_u64(position.start) != expected.start_frame
                    || sdk_u64(position.end) != expected.end_frame
                {
                    return Err(MutationError::VerifyFailed);
                }
                Ok(CreateMediaObjectResponse {
                    object: TimelineObject {
                        layer: sdk_u64(position.layer),
                        start_frame: sdk_u64(position.start),
                        end_frame: sdk_u64(position.end),
                        name: section
                            .get_object_name(handle)
                            .map_err(|_| MutationError::VerifyFailed)?,
                    },
                })
            })
            .map_err(|_| MutationError::Unavailable)?;
        write_media_mutation_debug(media_path, &result);
        result
    })
}

#[cfg(windows)]
fn write_media_mutation_debug(
    media_path: &std::path::Path,
    result: &Result<CreateMediaObjectResponse, MutationError>,
) {
    let Ok(path) = std::env::var(MUTATION_DEBUG_LOG_ENV) else {
        return;
    };
    let Some(file_name) = media_path.file_name() else {
        return;
    };
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let record = serde_json::json!({
        "operation": "create_media",
        "fileName": file_name.to_string_lossy(),
        "outcome": if result.is_ok() { "succeeded" } else { "failed" },
    });
    let _ = writeln!(file, "{record}");
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

#[cfg(not(windows))]
fn platform_objects_reader() -> Arc<ObjectsReader> {
    Arc::new(|| Err(EditorError::Unavailable))
}

#[cfg(not(windows))]
fn platform_object_details_reader() -> Arc<ObjectDetailsReader> {
    Arc::new(|| Err(EditorError::Unavailable))
}

#[cfg(not(windows))]
fn platform_object_mover() -> Arc<ObjectMover> {
    Arc::new(|_| Err(MutationError::Unavailable))
}

#[cfg(not(windows))]
fn platform_object_deleter() -> Arc<ObjectDeleter> {
    Arc::new(|_| Err(MutationError::Unavailable))
}

#[cfg(not(windows))]
fn platform_text_object_creator() -> Arc<TextObjectCreator> {
    Arc::new(|_| Err(MutationError::Unavailable))
}

#[cfg(not(windows))]
fn platform_text_object_updater() -> Arc<TextObjectUpdater> {
    Arc::new(|_| Err(TextUpdateError::Unavailable))
}

#[cfg(not(windows))]
fn platform_object_duplicator() -> Arc<ObjectDuplicator> {
    Arc::new(|_| Err(MutationError::Unavailable))
}

#[cfg(not(windows))]
fn platform_media_object_creator() -> Arc<MediaObjectCreator> {
    Arc::new(|_| Err(MutationError::Unavailable))
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

    use super::{
        ACCEPT_POLL, ApiServer, EditorError, ServerError, ServerParts, configure_accepted_stream,
        handle_connection, worker_loop,
    };

    fn start(
        reader: impl Fn() -> Result<super::SceneRead, EditorError> + Send + Sync + 'static,
    ) -> ApiServer {
        start_with_mutators(
            reader,
            |_| Err(super::MutationError::Unavailable),
            |_| Err(super::MutationError::Unavailable),
        )
    }

    #[test]
    fn accepted_stream_is_blocking_before_waiting_for_request_bytes() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let client = thread::spawn(move || {
            let mut stream = TcpStream::connect(address).unwrap();
            thread::sleep(Duration::from_millis(25));
            stream.write_all(b"x").unwrap();
        });
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_nonblocking(true).unwrap();

        configure_accepted_stream(&stream).unwrap();
        let mut byte = [0_u8; 1];
        stream.read_exact(&mut byte).unwrap();

        assert_eq!(byte, *b"x");
        client.join().unwrap();
    }

    fn start_with_mover(
        reader: impl Fn() -> Result<super::SceneRead, EditorError> + Send + Sync + 'static,
        mover: impl Fn(
            &aviutl2_ai_agent_protocol::MoveObjectRequest,
        )
            -> Result<aviutl2_ai_agent_protocol::MoveObjectResponse, super::MutationError>
        + Send
        + Sync
        + 'static,
    ) -> ApiServer {
        start_with_mutators(reader, mover, |_| Err(super::MutationError::Unavailable))
    }

    fn start_with_deleter(
        reader: impl Fn() -> Result<super::SceneRead, EditorError> + Send + Sync + 'static,
        deleter: impl Fn(
            &aviutl2_ai_agent_protocol::DeleteObjectRequest,
        ) -> Result<
            aviutl2_ai_agent_protocol::DeleteObjectResponse,
            super::MutationError,
        > + Send
        + Sync
        + 'static,
    ) -> ApiServer {
        start_with_mutators(reader, |_| Err(super::MutationError::Unavailable), deleter)
    }

    fn start_with_mutators(
        reader: impl Fn() -> Result<super::SceneRead, EditorError> + Send + Sync + 'static,
        mover: impl Fn(
            &aviutl2_ai_agent_protocol::MoveObjectRequest,
        )
            -> Result<aviutl2_ai_agent_protocol::MoveObjectResponse, super::MutationError>
        + Send
        + Sync
        + 'static,
        deleter: impl Fn(
            &aviutl2_ai_agent_protocol::DeleteObjectRequest,
        ) -> Result<
            aviutl2_ai_agent_protocol::DeleteObjectResponse,
            super::MutationError,
        > + Send
        + Sync
        + 'static,
    ) -> ApiServer {
        ApiServer::start_with_parts(
            "127.0.0.1:0",
            4,
            ServerParts {
                scene_reader: Arc::new(reader),
                timeline_reader: Arc::new(|| Ok(timeline())),
                objects_reader: Arc::new(|| Ok(objects())),
                object_details_reader: Arc::new(|| Err(EditorError::Unavailable)),
                object_mover: Arc::new(mover),
                object_deleter: Arc::new(deleter),
                text_object_creator: Arc::new(|_| Err(super::MutationError::Unavailable)),
                text_object_updater: Arc::new(|_| Err(super::TextUpdateError::Unavailable)),
                object_duplicator: Arc::new(|_| Err(super::MutationError::Unavailable)),
                media_object_creator: Arc::new(|_| Err(super::MutationError::Unavailable)),
            },
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

    fn objects() -> aviutl2_ai_agent_protocol::CurrentObjects {
        aviutl2_ai_agent_protocol::CurrentObjects {
            objects: vec![aviutl2_ai_agent_protocol::TimelineObject {
                layer: 0,
                start_frame: 10,
                end_frame: 39,
                name: Some("Title".to_owned()),
            }],
        }
    }

    fn object_details() -> aviutl2_ai_agent_protocol::CurrentObjectDetails {
        aviutl2_ai_agent_protocol::CurrentObjectDetails {
            objects: vec![
                aviutl2_ai_agent_protocol::ObjectDetails {
                    object: objects().objects[0].clone(),
                    kind: aviutl2_ai_agent_protocol::ObjectKind::Text,
                    text: Some("Hello".to_owned()),
                },
                aviutl2_ai_agent_protocol::ObjectDetails {
                    object: aviutl2_ai_agent_protocol::TimelineObject {
                        layer: 1,
                        start_frame: 10,
                        end_frame: 39,
                        name: None,
                    },
                    kind: aviutl2_ai_agent_protocol::ObjectKind::Text,
                    text: None,
                },
            ],
        }
    }

    fn start_with_object_details_reader(
        reader: impl Fn() -> Result<aviutl2_ai_agent_protocol::CurrentObjectDetails, EditorError>
        + Send
        + Sync
        + 'static,
    ) -> ApiServer {
        ApiServer::start_with_parts(
            "127.0.0.1:0",
            4,
            ServerParts {
                scene_reader: Arc::new(|| Ok(scene("Root", 0))),
                timeline_reader: Arc::new(|| Ok(timeline())),
                objects_reader: Arc::new(|| Ok(objects())),
                object_details_reader: Arc::new(reader),
                object_mover: Arc::new(|_| Err(super::MutationError::Unavailable)),
                object_deleter: Arc::new(|_| Err(super::MutationError::Unavailable)),
                text_object_creator: Arc::new(|_| Err(super::MutationError::Unavailable)),
                text_object_updater: Arc::new(|_| Err(super::TextUpdateError::Unavailable)),
                object_duplicator: Arc::new(|_| Err(super::MutationError::Unavailable)),
                media_object_creator: Arc::new(|_| Err(super::MutationError::Unavailable)),
            },
            |_, listener, shutting_down, context| {
                thread::Builder::new().spawn(move || worker_loop(listener, shutting_down, context))
            },
        )
        .unwrap()
    }

    fn start_with_text_updater(
        updater: impl Fn(
            &aviutl2_ai_agent_protocol::UpdateTextObjectRequest,
        ) -> Result<
            aviutl2_ai_agent_protocol::UpdateTextObjectResponse,
            super::TextUpdateError,
        > + Send
        + Sync
        + 'static,
    ) -> ApiServer {
        ApiServer::start_with_parts(
            "127.0.0.1:0",
            4,
            ServerParts {
                scene_reader: Arc::new(|| Ok(scene("Root", 0))),
                timeline_reader: Arc::new(|| Ok(timeline())),
                objects_reader: Arc::new(|| Ok(objects())),
                object_details_reader: Arc::new(|| Ok(object_details())),
                object_mover: Arc::new(|_| Err(super::MutationError::Unavailable)),
                object_deleter: Arc::new(|_| Err(super::MutationError::Unavailable)),
                text_object_creator: Arc::new(|_| Err(super::MutationError::Unavailable)),
                text_object_updater: Arc::new(updater),
                object_duplicator: Arc::new(|_| Err(super::MutationError::Unavailable)),
                media_object_creator: Arc::new(|_| Err(super::MutationError::Unavailable)),
            },
            |_, listener, shutting_down, context| {
                thread::Builder::new().spawn(move || worker_loop(listener, shutting_down, context))
            },
        )
        .unwrap()
    }

    fn request(address: std::net::SocketAddr, path: &str) -> String {
        raw_request(
            address,
            &format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: keep-alive\r\n\r\n"),
        )
    }

    fn start_with_text_creator(
        creator: impl Fn(
            &aviutl2_ai_agent_protocol::CreateTextObjectRequest,
        ) -> Result<
            aviutl2_ai_agent_protocol::CreateTextObjectResponse,
            super::MutationError,
        > + Send
        + Sync
        + 'static,
    ) -> ApiServer {
        ApiServer::start_with_parts(
            "127.0.0.1:0",
            4,
            ServerParts {
                scene_reader: Arc::new(|| Ok(scene("Root", 0))),
                timeline_reader: Arc::new(|| Ok(timeline())),
                objects_reader: Arc::new(|| Ok(objects())),
                object_details_reader: Arc::new(|| Err(EditorError::Unavailable)),
                object_mover: Arc::new(|_| Err(super::MutationError::Unavailable)),
                object_deleter: Arc::new(|_| Err(super::MutationError::Unavailable)),
                text_object_creator: Arc::new(creator),
                text_object_updater: Arc::new(|_| Err(super::TextUpdateError::Unavailable)),
                object_duplicator: Arc::new(|_| Err(super::MutationError::Unavailable)),
                media_object_creator: Arc::new(|_| Err(super::MutationError::Unavailable)),
            },
            |_, listener, shutting_down, context| {
                thread::Builder::new().spawn(move || worker_loop(listener, shutting_down, context))
            },
        )
        .unwrap()
    }

    fn start_with_media_creator(
        creator: impl Fn(
            &aviutl2_ai_agent_protocol::CreateMediaObjectRequest,
        ) -> Result<
            aviutl2_ai_agent_protocol::CreateMediaObjectResponse,
            super::MutationError,
        > + Send
        + Sync
        + 'static,
    ) -> ApiServer {
        ApiServer::start_with_parts(
            "127.0.0.1:0",
            4,
            ServerParts {
                scene_reader: Arc::new(|| Ok(scene("Root", 0))),
                timeline_reader: Arc::new(|| Ok(timeline())),
                objects_reader: Arc::new(|| Ok(objects())),
                object_details_reader: Arc::new(|| Err(EditorError::Unavailable)),
                object_mover: Arc::new(|_| Err(super::MutationError::Unavailable)),
                object_deleter: Arc::new(|_| Err(super::MutationError::Unavailable)),
                text_object_creator: Arc::new(|_| Err(super::MutationError::Unavailable)),
                text_object_updater: Arc::new(|_| Err(super::TextUpdateError::Unavailable)),
                object_duplicator: Arc::new(|_| Err(super::MutationError::Unavailable)),
                media_object_creator: Arc::new(creator),
            },
            |_, listener, shutting_down, context| {
                thread::Builder::new().spawn(move || worker_loop(listener, shutting_down, context))
            },
        )
        .unwrap()
    }

    fn raw_request(address: std::net::SocketAddr, request: &str) -> String {
        let mut stream = TcpStream::connect(address).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        read_complete_http_response(&mut stream)
    }

    fn read_complete_http_response(stream: &mut TcpStream) -> String {
        let mut response = Vec::new();
        let mut chunk = [0_u8; 512];
        loop {
            let count = stream.read(&mut chunk).unwrap();
            assert_ne!(count, 0, "connection closed before the response completed");
            response.extend_from_slice(&chunk[..count]);
            let Some(head_end) = response
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map(|position| position + 4)
            else {
                continue;
            };
            let head = std::str::from_utf8(&response[..head_end]).unwrap();
            let content_length = super::parse_content_length(head).unwrap();
            if response.len() >= head_end + content_length {
                response.truncate(head_end + content_length);
                return String::from_utf8(response).unwrap();
            }
        }
    }

    fn post(address: std::net::SocketAddr, path: &str, body: &str) -> String {
        raw_request(
            address,
            &format!(
                "POST {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        )
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
    fn current_objects_use_handle_free_snapshot_dto() {
        let server = start(|| Ok(scene("Root", 0)));
        let response = request(server.local_addr(), "/v1/scenes/current/objects");
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        let actual: aviutl2_ai_agent_protocol::CurrentObjects =
            serde_json::from_str(body(&response)).unwrap();
        assert_eq!(actual, objects());
    }

    #[test]
    fn current_object_details_return_kind_and_text_without_sdk_names() {
        let server = start_with_object_details_reader(|| Ok(object_details()));
        let response = request(server.local_addr(), "/v1/scenes/current/objects/details");
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        let actual: aviutl2_ai_agent_protocol::CurrentObjectDetails =
            serde_json::from_str(body(&response)).unwrap();
        assert_eq!(actual, object_details());
        assert!(!body(&response).contains("テキスト"));
    }

    #[test]
    fn move_endpoint_parses_body_and_returns_handle_free_result() {
        let server = start_with_mover(
            || Ok(scene("Root", 0)),
            |request| {
                assert_eq!(request.expected_scene_name, "Root");
                Ok(aviutl2_ai_agent_protocol::MoveObjectResponse {
                    object: aviutl2_ai_agent_protocol::TimelineObject {
                        layer: request.destination.layer,
                        start_frame: request.destination.start_frame,
                        end_frame: request.destination.start_frame + 29,
                        name: request.target.name.clone(),
                    },
                })
            },
        );
        let request = r#"{"expectedSceneName":"Root","target":{"layer":0,"startFrame":10,"endFrame":39,"name":"Title"},"destination":{"layer":2,"startFrame":100}}"#;
        let response = post(
            server.local_addr(),
            "/v1/scenes/current/objects/move",
            request,
        );
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        let moved: aviutl2_ai_agent_protocol::MoveObjectResponse =
            serde_json::from_str(body(&response)).unwrap();
        assert_eq!(moved.object.layer, 2);
        assert_eq!(moved.object.start_frame, 100);
        assert_eq!(moved.object.end_frame, 129);
    }

    #[test]
    fn move_endpoint_rejects_invalid_and_conflicting_requests() {
        let server = start_with_mover(
            || Ok(scene("Root", 0)),
            |_| Err(super::MutationError::SceneConflict),
        );
        let invalid = post(server.local_addr(), "/v1/scenes/current/objects/move", "{}");
        assert!(invalid.starts_with("HTTP/1.1 400 Bad Request\r\n"));

        let request = r#"{"expectedSceneName":"Other","target":{"layer":0,"startFrame":10,"endFrame":39,"name":"Title"},"destination":{"layer":2,"startFrame":100}}"#;
        let conflict = post(
            server.local_addr(),
            "/v1/scenes/current/objects/move",
            request,
        );
        assert!(conflict.starts_with("HTTP/1.1 409 Conflict\r\n"));
        let error: ApiError = serde_json::from_str(body(&conflict)).unwrap();
        assert_eq!(error.code, ErrorCode::StateConflict);
        assert!(!error.retryable);
    }

    #[test]
    fn move_endpoint_marks_unconfirmed_outcome_for_reconciliation() {
        let server = start_with_mover(
            || Ok(scene("Root", 0)),
            |_| Err(super::MutationError::VerifyFailed),
        );
        let request = r#"{"expectedSceneName":"Root","target":{"layer":0,"startFrame":10,"endFrame":39,"name":"Title"},"destination":{"layer":2,"startFrame":100}}"#;
        let response = post(
            server.local_addr(),
            "/v1/scenes/current/objects/move",
            request,
        );
        assert!(response.starts_with("HTTP/1.1 500 Internal Server Error\r\n"));
        let error: ApiError = serde_json::from_str(body(&response)).unwrap();
        assert_eq!(error.code, ErrorCode::MutationOutcomeUnknown);
        assert!(error.message.contains("re-read current objects"));
        assert!(!error.retryable);
    }

    #[test]
    fn delete_endpoint_returns_deleted_snapshot() {
        let server = start_with_deleter(
            || Ok(scene("Root", 0)),
            |request| {
                assert_eq!(request.expected_scene_name, "Root");
                Ok(aviutl2_ai_agent_protocol::DeleteObjectResponse {
                    deleted: request.target.clone(),
                })
            },
        );
        let request = r#"{"expectedSceneName":"Root","target":{"layer":0,"startFrame":10,"endFrame":39,"name":"Title"}}"#;
        let response = post(
            server.local_addr(),
            "/v1/scenes/current/objects/delete",
            request,
        );
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        let deleted: aviutl2_ai_agent_protocol::DeleteObjectResponse =
            serde_json::from_str(body(&response)).unwrap();
        assert_eq!(deleted.deleted, objects().objects[0]);
    }

    #[test]
    fn delete_endpoint_maps_missing_target() {
        let server = start_with_deleter(
            || Ok(scene("Root", 0)),
            |_| {
                Err(super::MutationError::Validation(
                    super::MoveValidationError::TargetNotFound,
                ))
            },
        );
        let request = r#"{"expectedSceneName":"Root","target":{"layer":0,"startFrame":10,"endFrame":39,"name":"Title"}}"#;
        let response = post(
            server.local_addr(),
            "/v1/scenes/current/objects/delete",
            request,
        );
        assert!(response.starts_with("HTTP/1.1 404 Not Found\r\n"));
        let error: ApiError = serde_json::from_str(body(&response)).unwrap();
        assert_eq!(error.code, ErrorCode::ObjectNotFound);
    }

    #[test]
    fn create_text_endpoint_returns_verified_object_and_text() {
        let server = start_with_text_creator(|request| {
            Ok(aviutl2_ai_agent_protocol::CreateTextObjectResponse {
                object: aviutl2_ai_agent_protocol::TimelineObject {
                    layer: request.layer,
                    start_frame: request.start_frame,
                    end_frame: request.start_frame + request.length - 1,
                    name: None,
                },
                text: request.text.clone(),
            })
        });
        let request =
            r#"{"expectedSceneName":"Root","layer":1,"startFrame":100,"length":30,"text":"Hello"}"#;
        let response = post(
            server.local_addr(),
            "/v1/scenes/current/objects/text",
            request,
        );
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        let created: aviutl2_ai_agent_protocol::CreateTextObjectResponse =
            serde_json::from_str(body(&response)).unwrap();
        assert_eq!(created.object.end_frame, 129);
        assert_eq!(created.text, "Hello");
    }

    #[test]
    fn create_text_endpoint_rejects_zero_length_and_line_breaks() {
        let server = start_with_text_creator(|_| panic!("invalid request reached creator"));
        for request in [
            r#"{"expectedSceneName":"Root","layer":1,"startFrame":100,"length":0,"text":"Hello"}"#,
            "{\"expectedSceneName\":\"Root\",\"layer\":1,\"startFrame\":100,\"length\":30,\"text\":\"first\\nsecond\"}",
        ] {
            let response = post(
                server.local_addr(),
                "/v1/scenes/current/objects/text",
                request,
            );
            assert!(response.starts_with("HTTP/1.1 400 Bad Request\r\n"));
        }
    }

    #[test]
    fn update_text_endpoint_requires_expected_text_and_returns_read_back() {
        let server = start_with_text_updater(|request| {
            assert_eq!(request.expected_text, "Hello");
            Ok(aviutl2_ai_agent_protocol::UpdateTextObjectResponse {
                object: request.target.clone(),
                text: request.text.clone(),
            })
        });
        let request = r#"{"expectedSceneName":"Root","target":{"layer":0,"startFrame":10,"endFrame":39,"name":"Title"},"expectedText":"Hello","text":"Updated"}"#;
        let response = post(
            server.local_addr(),
            "/v1/scenes/current/objects/text/update",
            request,
        );
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        let updated: aviutl2_ai_agent_protocol::UpdateTextObjectResponse =
            serde_json::from_str(body(&response)).unwrap();
        assert_eq!(updated.text, "Updated");
        assert_eq!(updated.object, objects().objects[0]);
    }

    #[test]
    fn update_text_endpoint_rejects_stale_text_and_line_breaks() {
        let server = start_with_text_updater(|_| Err(super::TextUpdateError::TextConflict));
        let stale = r#"{"expectedSceneName":"Root","target":{"layer":0,"startFrame":10,"endFrame":39,"name":"Title"},"expectedText":"Old","text":"Updated"}"#;
        let response = post(
            server.local_addr(),
            "/v1/scenes/current/objects/text/update",
            stale,
        );
        assert!(response.starts_with("HTTP/1.1 409 Conflict\r\n"));

        let invalid = "{\"expectedSceneName\":\"Root\",\"target\":{\"layer\":0,\"startFrame\":10,\"endFrame\":39,\"name\":\"Title\"},\"expectedText\":\"Hello\",\"text\":\"first\\nsecond\"}";
        let response = post(
            server.local_addr(),
            "/v1/scenes/current/objects/text/update",
            invalid,
        );
        assert!(response.starts_with("HTTP/1.1 400 Bad Request\r\n"));
        let error: ApiError = serde_json::from_str(body(&response)).unwrap();
        assert!(error.message.contains("CR, LF, or NUL"));
    }

    #[test]
    fn update_text_endpoint_rejects_non_text_and_ambiguous_snapshot() {
        let request = r#"{"expectedSceneName":"Root","target":{"layer":0,"startFrame":10,"endFrame":39,"name":"Title"},"expectedText":"Hello","text":"Updated"}"#;
        for error in [
            super::TextUpdateError::NotTextObject,
            super::TextUpdateError::Validation(super::MoveValidationError::TargetAmbiguous),
        ] {
            let server = start_with_text_updater(move |_| Err(error));
            let response = post(
                server.local_addr(),
                "/v1/scenes/current/objects/text/update",
                request,
            );
            assert!(response.starts_with("HTTP/1.1 409 Conflict\r\n"));
            let error: ApiError = serde_json::from_str(body(&response)).unwrap();
            assert_eq!(error.code, ErrorCode::StateConflict);
            assert!(!error.retryable);
        }
    }

    #[test]
    fn update_text_endpoint_maps_sdk_read_failure_to_unavailable() {
        let server = start_with_text_updater(|_| Err(super::TextUpdateError::Unavailable));
        let request = r#"{"expectedSceneName":"Root","target":{"layer":0,"startFrame":10,"endFrame":39,"name":"Title"},"expectedText":"Hello","text":"Updated"}"#;
        let response = post(
            server.local_addr(),
            "/v1/scenes/current/objects/text/update",
            request,
        );
        assert!(response.starts_with("HTTP/1.1 503 Service Unavailable\r\n"));
        let error: ApiError = serde_json::from_str(body(&response)).unwrap();
        assert_eq!(error.code, ErrorCode::EditorUnavailable);
        assert!(error.retryable);
    }

    #[test]
    fn create_media_endpoint_returns_verified_object() {
        let server = start_with_media_creator(|request| {
            Ok(aviutl2_ai_agent_protocol::CreateMediaObjectResponse {
                object: aviutl2_ai_agent_protocol::TimelineObject {
                    layer: request.layer,
                    start_frame: request.start_frame,
                    end_frame: request.start_frame + request.length - 1,
                    name: Some("example.png".to_owned()),
                },
            })
        });
        let request = r#"{"expectedSceneName":"Root","mediaPath":"C:\\media\\example.png","layer":1,"startFrame":100,"length":90}"#;
        let response = post(
            server.local_addr(),
            "/v1/scenes/current/objects/media",
            request,
        );
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        let created: aviutl2_ai_agent_protocol::CreateMediaObjectResponse =
            serde_json::from_str(body(&response)).unwrap();
        assert_eq!(created.object.end_frame, 189);
    }

    #[test]
    fn create_media_endpoint_rejects_zero_length() {
        let server = start_with_media_creator(|_| panic!("invalid request reached creator"));
        let request = r#"{"expectedSceneName":"Root","mediaPath":"C:\\media\\example.png","layer":1,"startFrame":100,"length":0}"#;
        let response = post(
            server.local_addr(),
            "/v1/scenes/current/objects/media",
            request,
        );
        assert!(response.starts_with("HTTP/1.1 400 Bad Request\r\n"));
    }

    #[test]
    fn move_endpoint_supports_expect_continue() {
        let server = start_with_mover(
            || Ok(scene("Root", 0)),
            |_| Err(super::MutationError::SceneConflict),
        );
        let body_text = r#"{"expectedSceneName":"Other","target":{"layer":0,"startFrame":10,"endFrame":39,"name":"Title"},"destination":{"layer":2,"startFrame":100}}"#;
        let mut stream = TcpStream::connect(server.local_addr()).unwrap();
        let request_head = format!(
            "POST /v1/scenes/current/objects/move HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nExpect: 100-continue\r\n\r\n",
            server.local_addr(),
            body_text.len()
        );
        stream.write_all(request_head.as_bytes()).unwrap();
        let mut interim = [0_u8; 25];
        stream.read_exact(&mut interim).unwrap();
        assert_eq!(&interim, b"HTTP/1.1 100 Continue\r\n\r\n");
        stream.write_all(body_text.as_bytes()).unwrap();
        let response = read_complete_http_response(&mut stream);
        assert!(response.starts_with("HTTP/1.1 409 Conflict\r\n"));
    }

    #[test]
    fn oversized_expect_continue_is_rejected_before_interim_response() {
        let server = start(|| Ok(scene("Root", 0)));
        let mut stream = TcpStream::connect(server.local_addr()).unwrap();
        let request_head = format!(
            "POST /v1/scenes/current/objects/move HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nExpect: 100-continue\r\n\r\n",
            server.local_addr(),
            super::MAX_REQUEST_BODY + 1
        );
        stream.write_all(request_head.as_bytes()).unwrap();
        let response = read_complete_http_response(&mut stream);
        assert!(response.starts_with("HTTP/1.1 413 Payload Too Large\r\n"));
        assert!(!response.contains("100 Continue"));
        let error: ApiError = serde_json::from_str(body(&response)).unwrap();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert!(!error.retryable);
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
            ServerParts {
                scene_reader: Arc::new(|| Ok(scene("Root", 0))),
                timeline_reader: Arc::new(|| Ok(timeline())),
                objects_reader: Arc::new(|| Ok(objects())),
                object_details_reader: Arc::new(|| Err(EditorError::Unavailable)),
                object_mover: Arc::new(|_| Err(super::MutationError::Unavailable)),
                object_deleter: Arc::new(|_| Err(super::MutationError::Unavailable)),
                text_object_creator: Arc::new(|_| Err(super::MutationError::Unavailable)),
                text_object_updater: Arc::new(|_| Err(super::TextUpdateError::Unavailable)),
                object_duplicator: Arc::new(|_| Err(super::MutationError::Unavailable)),
                media_object_creator: Arc::new(|_| Err(super::MutationError::Unavailable)),
            },
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
            ServerParts {
                scene_reader: Arc::new(|| Ok(scene("Root", 0))),
                timeline_reader: Arc::new(|| Ok(timeline())),
                objects_reader: Arc::new(|| Ok(objects())),
                object_details_reader: Arc::new(|| Err(EditorError::Unavailable)),
                object_mover: Arc::new(|_| Err(super::MutationError::Unavailable)),
                object_deleter: Arc::new(|_| Err(super::MutationError::Unavailable)),
                text_object_creator: Arc::new(|_| Err(super::MutationError::Unavailable)),
                text_object_updater: Arc::new(|_| Err(super::TextUpdateError::Unavailable)),
                object_duplicator: Arc::new(|_| Err(super::MutationError::Unavailable)),
                media_object_creator: Arc::new(|_| Err(super::MutationError::Unavailable)),
            },
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
