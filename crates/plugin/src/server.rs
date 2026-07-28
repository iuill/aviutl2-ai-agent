use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

#[cfg(windows)]
use aviutl2_ai_agent_protocol::ReadSectionProbe;
use aviutl2_ai_agent_protocol::{Health, HealthStatus};

#[cfg(windows)]
use std::time::Instant;

const MAX_REQUEST_HEAD: usize = 8 * 1024;
const IO_TIMEOUT: Duration = Duration::from_millis(250);
const ACCEPT_POLL: Duration = Duration::from_millis(5);

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("worker_count must be greater than zero")]
    NoWorkers,
    #[error("failed to bind health server: {0}")]
    Bind(#[source] std::io::Error),
    #[error("failed to configure health listener: {0}")]
    Configure(#[source] std::io::Error),
    #[error("failed to spawn health worker: {0}")]
    Spawn(#[source] std::io::Error),
}

/// Minimal Phase 0 HTTP server whose threads are all owned and joined here.
///
/// Every response closes its connection. This prevents an idle keep-alive task
/// from executing plugin code after AviUtl2 unloads the DLL.
pub struct HealthServer {
    address: SocketAddr,
    shutting_down: Arc<AtomicBool>,
    workers: Vec<JoinHandle<()>>,
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct ShutdownObservation {
    pub(crate) worker_count: usize,
    pub(crate) join_panics: usize,
}

impl HealthServer {
    pub fn start(address: &str, worker_count: usize) -> Result<Self, ServerError> {
        Self::start_with_spawner(address, worker_count, |index, listener, shutting_down| {
            thread::Builder::new()
                .name(format!("aviutl2-ai-agent-http-{index}"))
                .spawn(move || worker_loop(listener, shutting_down))
        })
    }

    fn start_with_spawner<F>(
        address: &str,
        worker_count: usize,
        mut spawn_worker: F,
    ) -> Result<Self, ServerError>
    where
        F: FnMut(usize, Arc<TcpListener>, Arc<AtomicBool>) -> std::io::Result<JoinHandle<()>>,
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
        let mut workers: Vec<JoinHandle<()>> = Vec::with_capacity(worker_count);

        for index in 0..worker_count {
            let worker =
                match spawn_worker(index, Arc::clone(&listener), Arc::clone(&shutting_down)) {
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

impl Drop for HealthServer {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn worker_loop(listener: Arc<TcpListener>, shutting_down: Arc<AtomicBool>) {
    while !shutting_down.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => handle_connection(stream),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL);
            }
            Err(_) => break,
        }
    }
}

fn handle_connection(mut stream: TcpStream) {
    if stream.set_read_timeout(Some(IO_TIMEOUT)).is_err() {
        return;
    }
    if stream.set_write_timeout(Some(IO_TIMEOUT)).is_err() {
        return;
    }
    let _ = stream.set_nodelay(true);

    let mut request = Vec::with_capacity(512);
    let mut chunk = [0_u8; 512];
    while request.len() < MAX_REQUEST_HEAD {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(count) => {
                request.extend_from_slice(&chunk[..count]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
        }
    }

    let request_line = request
        .split(|byte| *byte == b'\n')
        .next()
        .and_then(|line| std::str::from_utf8(line).ok())
        .unwrap_or_default()
        .trim_end_matches('\r');

    let (status, content_type, body) =
        if request_line == "GET /healthz HTTP/1.1" || request_line == "GET /healthz HTTP/1.0" {
            let health = Health {
                status: HealthStatus::Ok,
                plugin_version: env!("CARGO_PKG_VERSION").to_owned(),
            };
            (
                "200 OK",
                "application/json",
                serde_json::to_vec(&health).expect("Health is serializable"),
            )
        } else if request_line == "GET /phase0/read-section HTTP/1.1"
            || request_line == "GET /phase0/read-section HTTP/1.0"
        {
            #[cfg(windows)]
            {
                (
                    "200 OK",
                    "application/json",
                    serde_json::to_vec(&run_read_section_probe())
                        .expect("ReadSectionProbe is serializable"),
                )
            }
            #[cfg(not(windows))]
            {
                (
                    "404 Not Found",
                    "text/plain; charset=utf-8",
                    b"not found".to_vec(),
                )
            }
        } else {
            (
                "404 Not Found",
                "text/plain; charset=utf-8",
                b"not found".to_vec(),
            )
        };

    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&body);
    let _ = stream.flush();
}

#[cfg(windows)]
fn run_read_section_probe() -> ReadSectionProbe {
    let worker_thread = format!("{:?}", thread::current().id());
    if !crate::windows_plugin::EDIT_HANDLE.is_ready() {
        return ReadSectionProbe {
            success: false,
            worker_thread,
            callback_thread: None,
            elapsed_micros: 0,
            scene_name: None,
            error: Some("edit handle is not ready".into()),
        };
    }

    let started = Instant::now();
    let result = crate::windows_plugin::EDIT_HANDLE.call_read_section(|section| {
        let callback_thread = format!("{:?}", thread::current().id());
        (callback_thread, section.get_scene_name())
    });
    let elapsed_micros = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);

    match result {
        Ok((callback_thread, Ok(scene_name))) => ReadSectionProbe {
            success: true,
            worker_thread,
            callback_thread: Some(callback_thread),
            elapsed_micros,
            scene_name: Some(scene_name),
            error: None,
        },
        Ok((callback_thread, Err(error))) => ReadSectionProbe {
            success: false,
            worker_thread,
            callback_thread: Some(callback_thread),
            elapsed_micros,
            scene_name: None,
            error: Some(format!("read section callback failed: {error}")),
        },
        Err(error) => ReadSectionProbe {
            success: false,
            worker_thread,
            callback_thread: None,
            elapsed_micros,
            scene_name: None,
            error: Some(format!("call_read_section failed: {error}")),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        thread,
        time::{Duration, Instant},
    };

    use aviutl2_ai_agent_protocol::{Health, HealthStatus};

    use super::{ACCEPT_POLL, HealthServer, ServerError, handle_connection, worker_loop};

    fn request(address: std::net::SocketAddr, path: &str) -> String {
        let mut stream = TcpStream::connect(address).unwrap();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: keep-alive\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    #[test]
    fn health_and_not_found_responses_close_the_connection() {
        let server = HealthServer::start("127.0.0.1:0", 2).unwrap();

        let response = request(server.local_addr(), "/healthz");
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response.contains("\r\nConnection: close\r\n"));
        let body = response.split_once("\r\n\r\n").unwrap().1;
        let health: Health = serde_json::from_str(body).unwrap();
        assert_eq!(health.status, HealthStatus::Ok);

        let response = request(server.local_addr(), "/missing");
        assert!(response.starts_with("HTTP/1.1 404 Not Found\r\n"));
    }

    #[test]
    fn drop_joins_workers_and_releases_the_port() {
        let accepted = Arc::new(AtomicBool::new(false));
        let mut server = HealthServer::start_with_spawner("127.0.0.1:0", 1, {
            let accepted = Arc::clone(&accepted);
            move |_, listener, shutting_down| {
                let accepted = Arc::clone(&accepted);
                thread::Builder::new().spawn(move || {
                    while !shutting_down.load(Ordering::Acquire) {
                        match listener.accept() {
                            Ok((stream, _)) => {
                                accepted.store(true, Ordering::Release);
                                handle_connection(stream);
                            }
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                thread::sleep(ACCEPT_POLL);
                            }
                            Err(_) => break,
                        }
                    }
                })
            }
        })
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
            HealthServer::start("127.0.0.1:0", 0),
            Err(ServerError::NoWorkers)
        ));
    }

    #[test]
    fn spawn_failure_stops_started_workers_and_releases_port() {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = probe.local_addr().unwrap();
        drop(probe);

        let result = HealthServer::start_with_spawner(
            &address.to_string(),
            4,
            |index, listener, shutting_down| {
                if index == 2 {
                    return Err(std::io::Error::other("injected spawn failure"));
                }
                thread::Builder::new().spawn(move || worker_loop(listener, shutting_down))
            },
        );
        assert!(matches!(result, Err(ServerError::Spawn(_))));
        TcpListener::bind(address)
            .expect("startup rollback must stop workers and release listener");
    }
}
