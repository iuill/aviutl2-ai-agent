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

use aviutl2_agent_protocol::{Health, HealthStatus};

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

impl HealthServer {
    pub fn start(address: &str, worker_count: usize) -> Result<Self, ServerError> {
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
        let mut workers = Vec::with_capacity(worker_count);

        for index in 0..worker_count {
            let listener = Arc::clone(&listener);
            let shutting_down = Arc::clone(&shutting_down);
            let worker = thread::Builder::new()
                .name(format!("aviutl2-agent-http-{index}"))
                .spawn(move || worker_loop(listener, shutting_down))
                .map_err(ServerError::Spawn)?;
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

    fn shutdown(&mut self) {
        self.shutting_down.store(true, Ordering::Release);
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

impl Drop for HealthServer {
    fn drop(&mut self) {
        self.shutdown();
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
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
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

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        time::{Duration, Instant},
    };

    use aviutl2_agent_protocol::{Health, HealthStatus};

    use super::{HealthServer, ServerError};

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
        let server = HealthServer::start("127.0.0.1:0", 4).unwrap();
        let address = server.local_addr();
        let idle_keep_alive = TcpStream::connect(address).unwrap();
        let started = Instant::now();
        drop(server);
        drop(idle_keep_alive);

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
}
