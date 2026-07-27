use anyhow::{Context, Result, bail};
use aviutl2_agent_protocol::Health;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(version, about = "AviUtl2 agent Phase 0 probe")]
struct Args {
    #[arg(long, default_value = "http://127.0.0.1:7890")]
    endpoint: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Probe the plugin's gate-free health endpoint.
    Health,
}

fn main() -> Result<()> {
    let args = Args::parse();
    match args.command {
        Command::Health => println!(
            "{}",
            serde_json::to_string_pretty(&get_health(&args.endpoint)?)?
        ),
    }
    Ok(())
}

fn get_health(base_endpoint: &str) -> Result<Health> {
    let endpoint = format!("{}/healthz", base_endpoint.trim_end_matches('/'));
    let mut response = match ureq::get(&endpoint).call() {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(status)) => {
            bail!("health endpoint returned HTTP {status}");
        }
        Err(error) => {
            return Err(error).with_context(|| format!("failed to connect to {endpoint}"));
        }
    };
    response
        .body_mut()
        .read_json()
        .context("invalid health response")
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    use aviutl2_agent_protocol::HealthStatus;

    use super::get_health;

    fn serve_once(status: &str, body: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let status = status.to_owned();
        let body = body.to_owned();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        format!("http://{address}")
    }

    #[test]
    fn parses_health_response() {
        let endpoint = serve_once(
            "200 OK",
            r#"{"status":"ok","pluginVersion":"test-version"}"#,
        );
        let health = get_health(&endpoint).unwrap();
        assert_eq!(health.status, HealthStatus::Ok);
        assert_eq!(health.plugin_version, "test-version");
    }

    #[test]
    fn reports_http_status_without_claiming_connection_failure() {
        let endpoint = serve_once("404 Not Found", r#"{"error":"missing"}"#);
        let error = get_health(&endpoint).unwrap_err().to_string();
        assert_eq!(error, "health endpoint returned HTTP 404");
    }
}
