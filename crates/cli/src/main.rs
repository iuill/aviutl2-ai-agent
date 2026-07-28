use std::process::ExitCode;

use anyhow::Context;
use aviutl2_ai_agent_protocol::{ApiError, CurrentScene, ErrorCode, Health, Status};
use clap::{Parser, Subcommand};
use serde::de::DeserializeOwned;

#[derive(Debug, Parser)]
#[command(version, about = "AviUtl2 local read-only API client")]
struct Args {
    #[arg(long, default_value = "http://127.0.0.1:7890")]
    endpoint: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Check whether the plugin HTTP listener is alive.
    Health,
    /// Show SDK-independent plugin and listener status.
    Status,
    /// Read the currently selected scene.
    CurrentScene,
}

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::from(error.exit_code())
        }
    }
}

fn run(args: Args) -> Result<(), ClientError> {
    let output = match args.command {
        Command::Health => {
            serde_json::to_string_pretty(&get::<Health>(&args.endpoint, "/healthz", "health")?)
        }
        Command::Status => {
            serde_json::to_string_pretty(&get::<Status>(&args.endpoint, "/v1/status", "status")?)
        }
        Command::CurrentScene => serde_json::to_string_pretty(&get::<CurrentScene>(
            &args.endpoint,
            "/v1/scenes/current",
            "current scene",
        )?),
    }
    .context("failed to serialize response")
    .map_err(ClientError::Other)?;
    println!("{output}");
    Ok(())
}

#[derive(Debug, thiserror::Error)]
enum ClientError {
    #[error("API returned HTTP {status}: {error:?}")]
    Api { status: u16, error: ApiError },
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl ClientError {
    fn exit_code(&self) -> u8 {
        match self {
            Self::Api {
                error:
                    ApiError {
                        code: ErrorCode::InvalidRequest,
                        ..
                    },
                ..
            } => 2,
            Self::Api {
                error:
                    ApiError {
                        code: ErrorCode::EditorBusy | ErrorCode::EditorUnavailable,
                        ..
                    },
                ..
            } => 3,
            Self::Api { .. } | Self::Other(_) => 1,
        }
    }
}

fn get<T: DeserializeOwned>(
    base_endpoint: &str,
    path: &str,
    response_name: &str,
) -> Result<T, ClientError> {
    let endpoint = format!("{}{path}", base_endpoint.trim_end_matches('/'));
    let agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .new_agent();
    let mut response = agent
        .get(&endpoint)
        .call()
        .with_context(|| format!("failed to connect to {endpoint}"))
        .map_err(ClientError::Other)?;
    let status = response.status().as_u16();
    if !response.status().is_success() {
        let error = response
            .body_mut()
            .read_json::<ApiError>()
            .with_context(|| format!("invalid API error response from {endpoint}"))
            .map_err(ClientError::Other)?;
        return Err(ClientError::Api { status, error });
    }
    response
        .body_mut()
        .read_json()
        .with_context(|| format!("invalid {response_name} response"))
        .map_err(ClientError::Other)
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    use aviutl2_ai_agent_protocol::{CurrentScene, ErrorCode, HealthStatus, Status};

    use super::{ClientError, get};

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
    fn parses_status_and_current_scene_responses() {
        let endpoint = serve_once(
            "200 OK",
            r#"{"status":"ok","pluginVersion":"test","apiVersion":"v1","listenerAddress":"127.0.0.1:7890","processId":42}"#,
        );
        let status: Status = get(&endpoint, "/v1/status", "status").unwrap();
        assert_eq!(status.status, HealthStatus::Ok);
        assert_eq!(status.process_id, 42);

        let endpoint = serve_once("200 OK", r#"{"name":"Scene 1"}"#);
        let scene: CurrentScene = get(&endpoint, "/v1/scenes/current", "current scene").unwrap();
        assert_eq!(scene.name, "Scene 1");
    }

    #[test]
    fn maps_request_error_to_exit_code_two() {
        let endpoint = serve_once(
            "400 Bad Request",
            r#"{"code":"invalid_request","message":"bad request","retryable":false}"#,
        );
        let error = get::<Status>(&endpoint, "/v1/status", "status").unwrap_err();
        assert_eq!(error.exit_code(), 2);
        assert!(matches!(
            error,
            ClientError::Api {
                error: aviutl2_ai_agent_protocol::ApiError {
                    code: ErrorCode::InvalidRequest,
                    retryable: false,
                    ..
                },
                ..
            }
        ));
    }

    #[test]
    fn maps_retryable_editor_errors_to_exit_code_three() {
        for code in ["editor_busy", "editor_unavailable"] {
            let endpoint = serve_once(
                "503 Service Unavailable",
                &format!(r#"{{"code":"{code}","message":"try later","retryable":true}}"#),
            );
            let error =
                get::<CurrentScene>(&endpoint, "/v1/scenes/current", "current scene").unwrap_err();
            assert_eq!(error.exit_code(), 3);
        }
    }

    #[test]
    fn maps_non_retryable_api_and_transport_errors_to_exit_code_one() {
        let endpoint = serve_once(
            "404 Not Found",
            r#"{"code":"route_not_found","message":"missing","retryable":false}"#,
        );
        assert_eq!(
            get::<Status>(&endpoint, "/v1/status", "status")
                .unwrap_err()
                .exit_code(),
            1
        );

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        assert_eq!(
            get::<Status>(&endpoint, "/v1/status", "status")
                .unwrap_err()
                .exit_code(),
            1
        );
    }
}
