use std::process::ExitCode;

use anyhow::Context;
use aviutl2_ai_agent_protocol::{
    ApiError, CreateTextObjectRequest, CreateTextObjectResponse, CurrentObjects, CurrentScene,
    CurrentTimeline, DeleteObjectRequest, DeleteObjectResponse, DuplicateObjectRequest,
    DuplicateObjectResponse, ErrorCode, Health, MoveObjectDestination, MoveObjectRequest,
    MoveObjectResponse, Status, TimelineObject,
};
use clap::{Parser, Subcommand};
use serde::de::DeserializeOwned;
use ureq::http;

#[derive(Debug, Parser)]
#[command(version, about = "AviUtl2 local API client")]
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
    /// Read the current scene timeline summary.
    CurrentTimeline,
    /// List objects in the current scene as a point-in-time snapshot.
    CurrentObjects,
    /// Move one object identified by its complete current snapshot.
    MoveObject {
        #[arg(long)]
        expected_scene_name: String,
        #[arg(long)]
        layer: usize,
        #[arg(long)]
        start_frame: usize,
        #[arg(long)]
        end_frame: usize,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        destination_layer: usize,
        #[arg(long)]
        destination_start_frame: usize,
    },
    /// Delete one object identified by its complete current snapshot.
    DeleteObject {
        #[arg(long)]
        expected_scene_name: String,
        #[arg(long)]
        layer: usize,
        #[arg(long)]
        start_frame: usize,
        #[arg(long)]
        end_frame: usize,
        #[arg(long)]
        name: Option<String>,
    },
    /// Create one text object with a single alias-based SDK mutation.
    CreateText {
        #[arg(long)]
        expected_scene_name: String,
        #[arg(long)]
        layer: usize,
        #[arg(long)]
        start_frame: usize,
        #[arg(long)]
        length: usize,
        #[arg(long)]
        text: String,
    },
    /// Duplicate one object at a non-overlapping destination.
    DuplicateObject {
        #[arg(long)]
        expected_scene_name: String,
        #[arg(long)]
        layer: usize,
        #[arg(long)]
        start_frame: usize,
        #[arg(long)]
        end_frame: usize,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        destination_layer: usize,
        #[arg(long)]
        destination_start_frame: usize,
    },
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
        Command::CurrentTimeline => serde_json::to_string_pretty(&get::<CurrentTimeline>(
            &args.endpoint,
            "/v1/scenes/current/timeline",
            "current timeline",
        )?),
        Command::CurrentObjects => serde_json::to_string_pretty(&get::<CurrentObjects>(
            &args.endpoint,
            "/v1/scenes/current/objects",
            "current objects",
        )?),
        Command::MoveObject {
            expected_scene_name,
            layer,
            start_frame,
            end_frame,
            name,
            destination_layer,
            destination_start_frame,
        } => serde_json::to_string_pretty(&post::<MoveObjectResponse>(
            &args.endpoint,
            "/v1/scenes/current/objects/move",
            &MoveObjectRequest {
                expected_scene_name,
                target: TimelineObject {
                    layer,
                    start_frame,
                    end_frame,
                    name,
                },
                destination: MoveObjectDestination {
                    layer: destination_layer,
                    start_frame: destination_start_frame,
                },
            },
            "move object",
        )?),
        Command::DeleteObject {
            expected_scene_name,
            layer,
            start_frame,
            end_frame,
            name,
        } => serde_json::to_string_pretty(&post::<DeleteObjectResponse>(
            &args.endpoint,
            "/v1/scenes/current/objects/delete",
            &DeleteObjectRequest {
                expected_scene_name,
                target: TimelineObject {
                    layer,
                    start_frame,
                    end_frame,
                    name,
                },
            },
            "delete object",
        )?),
        Command::CreateText {
            expected_scene_name,
            layer,
            start_frame,
            length,
            text,
        } => serde_json::to_string_pretty(&post::<CreateTextObjectResponse>(
            &args.endpoint,
            "/v1/scenes/current/objects/text",
            &CreateTextObjectRequest {
                expected_scene_name,
                layer,
                start_frame,
                length,
                text,
            },
            "create text object",
        )?),
        Command::DuplicateObject {
            expected_scene_name,
            layer,
            start_frame,
            end_frame,
            name,
            destination_layer,
            destination_start_frame,
        } => serde_json::to_string_pretty(&post::<DuplicateObjectResponse>(
            &args.endpoint,
            "/v1/scenes/current/objects/duplicate",
            &DuplicateObjectRequest {
                expected_scene_name,
                target: TimelineObject {
                    layer,
                    start_frame,
                    end_frame,
                    name,
                },
                destination: MoveObjectDestination {
                    layer: destination_layer,
                    start_frame: destination_start_frame,
                },
            },
            "duplicate object",
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

fn post<T: DeserializeOwned>(
    base_endpoint: &str,
    path: &str,
    body: &impl serde::Serialize,
    response_name: &str,
) -> Result<T, ClientError> {
    let endpoint = format!("{}{path}", base_endpoint.trim_end_matches('/'));
    let agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .new_agent();
    let mut response = agent
        .post(&endpoint)
        .send_json(body)
        .with_context(|| format!("failed to connect to {endpoint}"))
        .map_err(ClientError::Other)?;
    decode_response(&endpoint, response_name, &mut response)
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
    decode_response(&endpoint, response_name, &mut response)
}

fn decode_response<T: DeserializeOwned>(
    endpoint: &str,
    response_name: &str,
    response: &mut http::Response<ureq::Body>,
) -> Result<T, ClientError> {
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

    use aviutl2_ai_agent_protocol::{
        CurrentObjects, CurrentScene, CurrentTimeline, ErrorCode, HealthStatus,
        MoveObjectDestination, MoveObjectRequest, MoveObjectResponse, Status, TimelineObject,
    };

    use super::{ClientError, get, post};

    fn serve_once(status: &str, body: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let status = status.to_owned();
        let body = body.to_owned();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 1024];
            loop {
                let count = stream.read(&mut chunk).unwrap();
                request.extend_from_slice(&chunk[..count]);
                let Some(head_end) = request
                    .windows(4)
                    .position(|window| window == b"\r\n\r\n")
                    .map(|position| position + 4)
                else {
                    continue;
                };
                let head = std::str::from_utf8(&request[..head_end]).unwrap();
                let content_length = head
                    .split("\r\n")
                    .filter_map(|line| line.split_once(':'))
                    .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                    .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                    .unwrap_or(0);
                if request.len() >= head_end + content_length {
                    break;
                }
            }
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

        let endpoint = serve_once(
            "200 OK",
            r#"{"width":1920,"height":1080,"frameRate":{"numerator":30,"denominator":1},"cursorFrame":12,"objectEndFrame":99,"highestObjectLayer":2}"#,
        );
        let timeline: CurrentTimeline =
            get(&endpoint, "/v1/scenes/current/timeline", "current timeline").unwrap();
        assert_eq!(timeline.cursor_frame, 12);
        assert_eq!(timeline.frame_rate.numerator, 30);

        let endpoint = serve_once(
            "200 OK",
            r#"{"objects":[{"layer":0,"startFrame":10,"endFrame":39,"name":"Title"}]}"#,
        );
        let objects: CurrentObjects =
            get(&endpoint, "/v1/scenes/current/objects", "current objects").unwrap();
        assert_eq!(objects.objects.len(), 1);
        assert_eq!(objects.objects[0].start_frame, 10);
    }

    #[test]
    fn posts_move_request_and_parses_result() {
        let endpoint = serve_once(
            "200 OK",
            r#"{"object":{"layer":2,"startFrame":100,"endFrame":129,"name":"Title"}}"#,
        );
        let moved: MoveObjectResponse = post(
            &endpoint,
            "/v1/scenes/current/objects/move",
            &MoveObjectRequest {
                expected_scene_name: "Root".to_owned(),
                target: TimelineObject {
                    layer: 0,
                    start_frame: 10,
                    end_frame: 39,
                    name: Some("Title".to_owned()),
                },
                destination: MoveObjectDestination {
                    layer: 2,
                    start_frame: 100,
                },
            },
            "move object",
        )
        .unwrap();
        assert_eq!(moved.object.layer, 2);
        assert_eq!(moved.object.end_frame, 129);
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
