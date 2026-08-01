use anyhow::Result;
use aviutl2_ai_agent_protocol::{
    ApiError, CreateMediaObjectRequest, CreateMediaObjectResponse, CreateTextObjectRequest,
    CreateTextObjectResponse, CurrentObjectDetails, CurrentObjects, CurrentScene, CurrentTimeline,
    DeleteObjectRequest, DeleteObjectResponse, DuplicateObjectRequest, DuplicateObjectResponse,
    ErrorCode, MoveObjectDestination, MoveObjectRequest, MoveObjectResponse, PositionProperties,
    TextPropertiesPatch, UpdateTextObjectRequest, UpdateTextObjectResponse,
};
use base64::Engine;
use clap::Parser;
use rmcp::{
    ErrorData as McpError, ServiceExt,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock},
    schemars, tool, tool_router,
    transport::stdio,
};
use serde::de::DeserializeOwned;
use std::{io::Read, time::Duration};

const MAX_ERROR_BODY_BYTES: usize = 8 * 1024;
const CURRENT_FRAME_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Parser)]
#[command(version, about = "MCP server for the AviUtl2 local API")]
struct Args {
    #[arg(long, default_value = "http://127.0.0.1:7890")]
    endpoint: String,
}

#[derive(Debug, Clone)]
struct Aviutl2Mcp {
    endpoint: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
struct EmptyParams {}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
struct DestinationParams {
    layer: u64,
    start_frame: u64,
}

impl From<DestinationParams> for MoveObjectDestination {
    fn from(value: DestinationParams) -> Self {
        Self {
            layer: value.layer,
            start_frame: value.start_frame,
        }
    }
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
struct MoveObjectParams {
    expected_scene_name: String,
    object_id: String,
    destination: DestinationParams,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
struct DeleteObjectParams {
    expected_scene_name: String,
    object_id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
struct CreateTextObjectParams {
    expected_scene_name: String,
    layer: u64,
    start_frame: u64,
    length: u64,
    text: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
struct UpdateTextObjectParams {
    expected_scene_name: String,
    object_id: String,
    patch: TextPropertiesPatchParams,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
struct PositionPropertiesParams {
    x: String,
    y: String,
    z: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
struct TextPropertiesPatchParams {
    content: Option<String>,
    font: Option<String>,
    size: Option<String>,
    position: Option<PositionPropertiesParams>,
    color: Option<String>,
}

impl From<PositionPropertiesParams> for PositionProperties {
    fn from(value: PositionPropertiesParams) -> Self {
        Self {
            x: value.x,
            y: value.y,
            z: value.z,
        }
    }
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
struct DuplicateObjectParams {
    expected_scene_name: String,
    object_id: String,
    destination: DestinationParams,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
struct CreateMediaObjectParams {
    expected_scene_name: String,
    media_path: String,
    layer: u64,
    start_frame: u64,
    length: u64,
}

#[tool_router(server_handler)]
impl Aviutl2Mcp {
    #[tool(
        description = "現在選択されているAviUtl2 sceneを読み取ります。",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn get_current_scene(
        &self,
        Parameters(_): Parameters<EmptyParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(self.get::<CurrentScene>("/v1/scenes/current"))
    }

    #[tool(
        description = "現在のsceneのtimeline概要を読み取ります。",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn get_current_timeline(
        &self,
        Parameters(_): Parameters<EmptyParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(self.get::<CurrentTimeline>("/v1/scenes/current/timeline"))
    }

    #[tool(
        description = "現在のsceneにあるobjectのsnapshotを読み取ります。",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn list_current_objects(
        &self,
        Parameters(_): Parameters<EmptyParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(self.get::<CurrentObjects>("/v1/scenes/current/objects"))
    }

    #[tool(
        description = "現在のsceneにあるobjectのID、種別、layer/effect状態、textとmediaの設定を読み取ります。",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn list_current_object_details(
        &self,
        Parameters(_): Parameters<EmptyParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(self.get::<CurrentObjectDetails>("/v1/scenes/current/objects/details"))
    }

    #[tool(
        description = "現在frameをPNG画像として取得します。",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn get_current_frame(
        &self,
        Parameters(_): Parameters<EmptyParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(
            match get_bytes(&self.endpoint, "/v1/scenes/current/frame") {
                Ok(bytes) => CallToolResult::success(vec![ContentBlock::image(
                    base64::engine::general_purpose::STANDARD.encode(bytes),
                    "image/png",
                )]),
                Err(message) => CallToolResult::error(vec![ContentBlock::text(message)]),
            },
        )
    }

    #[tool(
        description = "list_current_objectsが返した現在のobject IDを使い、1つのobjectを指定位置へ移動します。自動再試行しないでください。",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn move_object(
        &self,
        Parameters(params): Parameters<MoveObjectParams>,
    ) -> Result<CallToolResult, McpError> {
        self.post::<_, MoveObjectResponse>(
            "/v1/scenes/current/objects/move",
            &MoveObjectRequest {
                expected_scene_name: params.expected_scene_name,
                object_id: params.object_id,
                destination: params.destination.into(),
            },
        )
    }

    #[tool(
        description = "list_current_objectsが返した現在のobject IDを使い、1つのobjectを削除します。自動再試行しないでください。",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn delete_object(
        &self,
        Parameters(params): Parameters<DeleteObjectParams>,
    ) -> Result<CallToolResult, McpError> {
        self.post::<_, DeleteObjectResponse>(
            "/v1/scenes/current/objects/delete",
            &DeleteObjectRequest {
                expected_scene_name: params.expected_scene_name,
                object_id: params.object_id,
            },
        )
    }

    #[tool(
        description = "plain text objectを1つ作成します。複数行の本文には文字列 \\n を使い、実際の改行文字は使わないでください。自動再試行しないでください。",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn create_text_object(
        &self,
        Parameters(params): Parameters<CreateTextObjectParams>,
    ) -> Result<CallToolResult, McpError> {
        self.post::<_, CreateTextObjectResponse>(
            "/v1/scenes/current/objects/text",
            &CreateTextObjectRequest {
                expected_scene_name: params.expected_scene_name,
                layer: params.layer,
                start_frame: params.start_frame,
                length: params.length,
                text: params.text,
            },
        )
    }

    #[tool(
        description = "list_current_object_detailsが返した現在のobject IDを使い、text objectの本文、font、size、XYZ位置、色を更新します。複数行の本文には文字列 \\n を使い、実際の改行文字は使わないでください。自動再試行しないでください。",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn update_text_object(
        &self,
        Parameters(params): Parameters<UpdateTextObjectParams>,
    ) -> Result<CallToolResult, McpError> {
        self.post::<_, UpdateTextObjectResponse>(
            "/v1/scenes/current/objects/text/update",
            &UpdateTextObjectRequest {
                expected_scene_name: params.expected_scene_name,
                object_id: params.object_id,
                patch: TextPropertiesPatch {
                    content: params.patch.content,
                    font: params.patch.font,
                    size: params.patch.size,
                    position: params.patch.position.map(Into::into),
                    color: params.patch.color,
                },
            },
        )
    }

    #[tool(
        description = "list_current_objectsが返した現在のobject IDを使い、1つのobjectを指定位置へ複製します。自動再試行しないでください。",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn duplicate_object(
        &self,
        Parameters(params): Parameters<DuplicateObjectParams>,
    ) -> Result<CallToolResult, McpError> {
        self.post::<_, DuplicateObjectResponse>(
            "/v1/scenes/current/objects/duplicate",
            &DuplicateObjectRequest {
                expected_scene_name: params.expected_scene_name,
                object_id: params.object_id,
                destination: params.destination.into(),
            },
        )
    }

    #[tool(
        description = "Windows上の絶対pathからmedia objectを1つ作成します。自動再試行しないでください。",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn create_media_object(
        &self,
        Parameters(params): Parameters<CreateMediaObjectParams>,
    ) -> Result<CallToolResult, McpError> {
        self.post::<_, CreateMediaObjectResponse>(
            "/v1/scenes/current/objects/media",
            &CreateMediaObjectRequest {
                expected_scene_name: params.expected_scene_name,
                media_path: params.media_path,
                layer: params.layer,
                start_frame: params.start_frame,
                length: params.length,
            },
        )
    }
}

impl Aviutl2Mcp {
    fn get<T: DeserializeOwned + serde::Serialize>(&self, path: &str) -> CallToolResult {
        match get::<T>(&self.endpoint, path) {
            Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
            Err(message) => CallToolResult::error(vec![ContentBlock::text(message)]),
        }
    }

    fn post<B: serde::Serialize, T: DeserializeOwned + serde::Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<CallToolResult, McpError> {
        Ok(match post::<B, T>(&self.endpoint, path, body) {
            Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
            Err(message) => CallToolResult::error(vec![ContentBlock::text(message)]),
        })
    }
}

fn get<T: DeserializeOwned + serde::Serialize>(
    base_endpoint: &str,
    path: &str,
) -> Result<String, String> {
    let endpoint = format!("{}{path}", base_endpoint.trim_end_matches('/'));
    let agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .new_agent();
    let mut response = agent
        .get(&endpoint)
        .call()
        .map_err(|error| error.to_string())?;
    decode_response::<T>(&mut response)
}

fn get_bytes(base_endpoint: &str, path: &str) -> Result<Vec<u8>, String> {
    let endpoint = format!("{}{path}", base_endpoint.trim_end_matches('/'));
    let agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(CURRENT_FRAME_TIMEOUT))
        .build()
        .new_agent();
    let mut response = agent
        .get(&endpoint)
        .call()
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        let status = response.status();
        let mut body = Vec::with_capacity(MAX_ERROR_BODY_BYTES + 1);
        response
            .body_mut()
            .as_reader()
            .take((MAX_ERROR_BODY_BYTES + 1) as u64)
            .read_to_end(&mut body)
            .map_err(|error| error.to_string())?;
        return Err(format_error_response(status, &body));
    }
    response
        .body_mut()
        .read_to_vec()
        .map_err(|error| error.to_string())
}

fn post<B: serde::Serialize, T: DeserializeOwned + serde::Serialize>(
    base_endpoint: &str,
    path: &str,
    body: &B,
) -> Result<String, String> {
    let endpoint = format!("{}{path}", base_endpoint.trim_end_matches('/'));
    let agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .new_agent();
    let mut response = agent
        .post(&endpoint)
        .send_json(body)
        .map_err(|error| error.to_string())?;
    decode_response::<T>(&mut response)
}

fn decode_response<T: DeserializeOwned + serde::Serialize>(
    response: &mut ureq::http::Response<ureq::Body>,
) -> Result<String, String> {
    if !response.status().is_success() {
        let status = response.status();
        let mut body = Vec::with_capacity(MAX_ERROR_BODY_BYTES + 1);
        response
            .body_mut()
            .as_reader()
            .take((MAX_ERROR_BODY_BYTES + 1) as u64)
            .read_to_end(&mut body)
            .map_err(|error| format!("AviUtl2 API error (HTTP {status}): {error}"))?;
        return Err(format_error_response(status, &body));
    }
    let value: T = response
        .body_mut()
        .read_json()
        .map_err(|error| error.to_string())?;
    serde_json::to_string_pretty(&value).map_err(|error| error.to_string())
}

fn format_error_response(status: ureq::http::StatusCode, body: &[u8]) -> String {
    if let Ok(error) = serde_json::from_slice::<ApiError>(body) {
        let reconcile = matches!(error.code, ErrorCode::MutationOutcomeUnknown);
        let error = serde_json::to_string_pretty(&error)
            .unwrap_or_else(|serialize_error| serialize_error.to_string());
        let guidance = if reconcile {
            "\nDo not retry this mutation. Call the relevant read tool to reconcile the actual state."
        } else {
            ""
        };
        return format!("AviUtl2 API error (HTTP {status}):\n{error}{guidance}");
    }

    let truncated = body.len() > MAX_ERROR_BODY_BYTES;
    let body = &body[..body.len().min(MAX_ERROR_BODY_BYTES)];
    let body = String::from_utf8_lossy(body);
    let body = if body.is_empty() {
        "<empty body>"
    } else {
        &body
    };
    let suffix = if truncated { "\n[truncated]" } else { "" };
    format!("AviUtl2 API error (HTTP {status}):\n{body}{suffix}")
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Args::parse();
    let service = Aviutl2Mcp {
        endpoint: args.endpoint,
    }
    .serve(stdio())
    .await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        borrow::Cow,
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    use rmcp::{
        ServerHandler, ServiceExt,
        model::{ProtocolVersion, ServerCapabilities},
    };
    use serde_json::Value;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    use super::{Aviutl2Mcp, MAX_ERROR_BODY_BYTES, format_error_response};

    fn server() -> Aviutl2Mcp {
        Aviutl2Mcp {
            endpoint: "http://127.0.0.1:1".to_owned(),
        }
    }

    #[test]
    fn tools_have_strict_schemas_and_accurate_annotations() {
        let tools = Aviutl2Mcp::tool_router().list_all();
        assert_eq!(tools.len(), 11);
        let mut tool_names = tools
            .iter()
            .map(|tool| tool.name.as_ref())
            .collect::<Vec<_>>();
        tool_names.sort_unstable();
        assert_eq!(
            tool_names,
            [
                "create_media_object",
                "create_text_object",
                "delete_object",
                "duplicate_object",
                "get_current_frame",
                "get_current_scene",
                "get_current_timeline",
                "list_current_object_details",
                "list_current_objects",
                "move_object",
                "update_text_object",
            ]
        );
        assert!(tools.iter().all(|tool| {
            tool.input_schema.get("additionalProperties") == Some(&Value::Bool(false))
        }));

        let read_tools = [
            "get_current_scene",
            "get_current_frame",
            "get_current_timeline",
            "list_current_object_details",
            "list_current_objects",
        ];
        for tool in tools
            .iter()
            .filter(|tool| read_tools.contains(&tool.name.as_ref()))
        {
            assert!(
                tool.input_schema
                    .get("properties")
                    .is_none_or(|value| value.as_object().is_some_and(serde_json::Map::is_empty))
            );
            let annotations = tool.annotations.as_ref().unwrap();
            assert_eq!(annotations.read_only_hint, Some(true));
            assert_eq!(annotations.destructive_hint, Some(false));
            assert_eq!(annotations.idempotent_hint, Some(true));
            assert_eq!(annotations.open_world_hint, Some(false));
        }

        for tool in tools
            .iter()
            .filter(|tool| !read_tools.contains(&tool.name.as_ref()))
        {
            let annotations = tool.annotations.as_ref().unwrap();
            assert_eq!(annotations.read_only_hint, Some(false));
            assert_eq!(annotations.idempotent_hint, Some(false));
            assert_eq!(annotations.open_world_hint, Some(false));
            assert!(
                tool.input_schema
                    .get("properties")
                    .is_some_and(Value::is_object)
            );
        }
        for name in ["move_object", "delete_object", "update_text_object"] {
            let tool = tools.iter().find(|tool| tool.name == name).unwrap();
            assert_eq!(
                tool.annotations.as_ref().unwrap().destructive_hint,
                Some(true)
            );
        }
        for name in [
            "create_text_object",
            "duplicate_object",
            "create_media_object",
        ] {
            let tool = tools.iter().find(|tool| tool.name == name).unwrap();
            assert_eq!(
                tool.annotations.as_ref().unwrap().destructive_hint,
                Some(false)
            );
        }
        assert!(
            tools
                .iter()
                .filter(|tool| !read_tools.contains(&tool.name.as_ref()))
                .all(|tool| {
                    tool.input_schema.get("required").is_some_and(|value| {
                        value.as_array().is_some_and(|values| !values.is_empty())
                    })
                })
        );
        for name in ["create_text_object", "update_text_object"] {
            let tool = tools.iter().find(|tool| tool.name == name).unwrap();
            assert!(tool.description.as_deref().unwrap().contains(r"文字列 \n"));
            assert!(
                tool.description
                    .as_deref()
                    .unwrap()
                    .contains("実際の改行文字は使わない")
            );
        }
        let details = tools
            .iter()
            .find(|tool| tool.name == "list_current_object_details")
            .unwrap();
        for term in ["ID", "種別", "状態", "text", "media"] {
            assert!(details.description.as_deref().unwrap().contains(term));
        }
    }

    #[test]
    fn api_errors_preserve_status_and_reconciliation_guidance() {
        let message = format_error_response(
            ureq::http::StatusCode::INTERNAL_SERVER_ERROR,
            br#"{"code":"mutation_outcome_unknown","message":"outcome unknown","retryable":false}"#,
        );
        assert!(message.contains("HTTP 500 Internal Server Error"));
        assert!(message.contains("mutation_outcome_unknown"));
        assert!(message.contains("\"retryable\": false"));
        assert!(message.contains("Do not retry this mutation"));
    }

    #[test]
    fn non_api_errors_preserve_status_and_bound_the_body() {
        let body = vec![b'x'; MAX_ERROR_BODY_BYTES + 100];
        let message = format_error_response(ureq::http::StatusCode::BAD_GATEWAY, &body);
        assert!(message.contains("HTTP 502 Bad Gateway"));
        assert!(message.ends_with("\n[truncated]"));
        assert_eq!(message.matches('x').count(), MAX_ERROR_BODY_BYTES);
    }

    #[test]
    fn advertises_tools_and_modern_and_legacy_protocols() {
        let info = server().get_info();
        assert_eq!(
            info.capabilities,
            ServerCapabilities::builder().enable_tools().build()
        );
        let versions: Cow<'static, [ProtocolVersion]> = server().supported_protocol_versions();
        assert!(versions.contains(&ProtocolVersion::V_2026_07_28));
        assert!(versions.contains(&ProtocolVersion::V_2025_06_18));
    }

    async fn exchange(first_request: &str) -> Value {
        let (client_transport, server_transport) = tokio::io::duplex(16 * 1024);
        let server_task = tokio::spawn(async move {
            server()
                .serve(server_transport)
                .await
                .expect("server should accept the opening request")
                .waiting()
                .await
                .expect("server should stop cleanly");
        });
        let (reader, mut writer) = tokio::io::split(client_transport);
        writer.write_all(first_request.as_bytes()).await.unwrap();
        writer.write_all(b"\n").await.unwrap();
        writer.flush().await.unwrap();
        let mut response = String::new();
        BufReader::new(reader)
            .read_line(&mut response)
            .await
            .unwrap();
        drop(writer);
        server_task.await.unwrap();
        serde_json::from_str(&response).unwrap()
    }

    fn serve_read_api() -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let task = thread::spawn(move || {
            let responses = [
                ("/v1/scenes/current", r#"{"name":"Root"}"#),
                (
                    "/v1/scenes/current/timeline",
                    r#"{"width":1920,"height":1080,"frameRate":{"numerator":30,"denominator":1},"cursorFrame":12,"objectEndFrame":39,"highestObjectLayer":0}"#,
                ),
                (
                    "/v1/scenes/current/objects",
                    r#"{"objects":[{"id":"obj-1","layer":0,"startFrame":10,"endFrame":39,"name":"Title"}]}"#,
                ),
                (
                    "/v1/scenes/current/objects/details",
                    r#"{"objects":[{"object":{"id":"obj-1","layer":0,"startFrame":10,"endFrame":39,"name":"Title"},"kind":"text","state":{"layerEnabled":true,"layerLocked":false,"effects":[]},"text":{"content":"Hello","font":"Yu Gothic UI","size":"40.00","position":{"x":"0.00","y":"0.00","z":"0.00"},"color":"ffffff"},"media":null}]}"#,
                ),
            ];
            for (path, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 4096];
                let length = stream.read(&mut request).unwrap();
                let request = std::str::from_utf8(&request[..length]).unwrap();
                assert!(request.starts_with(&format!("GET {path} HTTP/1.1\r\n")));
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
                stream.flush().unwrap();
            }
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let length = stream.read(&mut request).unwrap();
            let request = std::str::from_utf8(&request[..length]).unwrap();
            assert!(request.starts_with("GET /v1/scenes/current/frame HTTP/1.1\r\n"));
            let png = b"\x89PNG\r\n\x1a\nframe";
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                png.len()
            )
            .unwrap();
            stream.write_all(png).unwrap();
            stream.flush().unwrap();
        });
        (endpoint, task)
    }

    fn read_http_request(stream: &mut std::net::TcpStream) -> String {
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let length = stream.read(&mut buffer).unwrap();
            assert_ne!(length, 0, "connection closed before request completed");
            request.extend_from_slice(&buffer[..length]);
            let Some(header_end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") else {
                continue;
            };
            let header_end = header_end + 4;
            let headers = std::str::from_utf8(&request[..header_end]).unwrap();
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(str::trim)
                        .map(str::parse::<usize>)
                })
                .transpose()
                .unwrap()
                .unwrap_or(0);
            if request.len() >= header_end + content_length {
                return String::from_utf8(request).unwrap();
            }
        }
    }

    fn serve_write_api() -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let task = thread::spawn(move || {
            let responses = [
                (
                    "/v1/scenes/current/objects/move",
                    r#"{"expectedSceneName":"Root","objectId":"obj-1","destination":{"layer":2,"startFrame":100}}"#,
                    r#"{"object":{"id":"obj-2","layer":2,"startFrame":100,"endFrame":129,"name":"Title"}}"#,
                ),
                (
                    "/v1/scenes/current/objects/delete",
                    r#"{"expectedSceneName":"Root","objectId":"obj-2"}"#,
                    r#"{"deleted":{"id":"obj-2","layer":2,"startFrame":100,"endFrame":129,"name":"Title"}}"#,
                ),
                (
                    "/v1/scenes/current/objects/text",
                    r#"{"expectedSceneName":"Root","layer":1,"startFrame":100,"length":30,"text":"Hello"}"#,
                    r#"{"object":{"id":"obj-text","layer":1,"startFrame":100,"endFrame":129,"name":null},"text":"Hello"}"#,
                ),
                (
                    "/v1/scenes/current/objects/text/update",
                    r#"{"expectedSceneName":"Root","objectId":"obj-text","patch":{"content":"Updated","font":null,"size":null,"position":null,"color":null}}"#,
                    r#"{"object":{"id":"obj-text","layer":1,"startFrame":100,"endFrame":129,"name":null},"text":{"content":"Updated","font":"Yu Gothic UI","size":"40.00","position":{"x":"0.00","y":"0.00","z":"0.00"},"color":"ffffff"}}"#,
                ),
                (
                    "/v1/scenes/current/objects/duplicate",
                    r#"{"expectedSceneName":"Root","objectId":"obj-text","destination":{"layer":2,"startFrame":200}}"#,
                    r#"{"object":{"id":"obj-copy","layer":2,"startFrame":200,"endFrame":229,"name":null}}"#,
                ),
                (
                    "/v1/scenes/current/objects/media",
                    r#"{"expectedSceneName":"Root","mediaPath":"C:\\media\\image.png","layer":3,"startFrame":300,"length":90}"#,
                    r#"{"object":{"id":"obj-media","layer":3,"startFrame":300,"endFrame":389,"name":null}}"#,
                ),
            ];
            for (path, expected_body, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let request = read_http_request(&mut stream);
                assert!(request.starts_with(&format!("POST {path} HTTP/1.1\r\n")));
                let actual_body = request.split_once("\r\n\r\n").unwrap().1;
                assert_eq!(
                    serde_json::from_str::<Value>(actual_body).unwrap(),
                    serde_json::from_str::<Value>(expected_body).unwrap()
                );
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
                stream.flush().unwrap();
            }
        });
        (endpoint, task)
    }

    #[tokio::test]
    async fn supports_2026_discover_lifecycle() {
        let response = exchange(
            r#"{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientInfo":{"name":"test-client","version":"1.0.0"},"io.modelcontextprotocol/clientCapabilities":{}}}}"#,
        )
        .await;
        assert_eq!(response["result"]["resultType"], "complete");
        assert!(
            response["result"]["supportedVersions"]
                .as_array()
                .unwrap()
                .contains(&Value::String("2026-07-28".to_owned()))
        );
        assert!(response["result"]["capabilities"]["tools"].is_object());
    }

    #[tokio::test]
    async fn supports_legacy_initialize_lifecycle() {
        let response = exchange(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test-client","version":"1.0.0"}}}"#,
        )
        .await;
        assert_eq!(response["result"]["protocolVersion"], "2025-06-18");
        assert!(response["result"]["capabilities"]["tools"].is_object());
    }

    #[tokio::test]
    async fn stdio_read_tools_return_http_api_results() {
        let (endpoint, http_task) = serve_read_api();
        let (client_transport, server_transport) = tokio::io::duplex(16 * 1024);
        let server_task = tokio::spawn(async move {
            Aviutl2Mcp { endpoint }
                .serve(server_transport)
                .await
                .expect("server should initialize")
                .waiting()
                .await
                .expect("server should stop cleanly");
        });

        let (reader, mut writer) = tokio::io::split(client_transport);
        let mut reader = BufReader::new(reader);
        let initialize = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test-client","version":"1.0.0"}}}"#,
            "\n"
        );
        writer.write_all(initialize.as_bytes()).await.unwrap();
        writer.flush().await.unwrap();

        let mut response = String::new();
        reader.read_line(&mut response).await.unwrap();
        assert_eq!(serde_json::from_str::<Value>(&response).unwrap()["id"], 1);

        writer
            .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
            .await
            .unwrap();

        let calls = [
            (2, "get_current_scene", r#""name": "Root""#),
            (3, "get_current_timeline", r#""width": 1920"#),
            (4, "list_current_objects", r#""name": "Title""#),
            (5, "list_current_object_details", r#""content": "Hello""#),
        ];
        for (id, tool, expected) in calls {
            let call = format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"tools/call\",\"params\":{{\"name\":\"{tool}\",\"arguments\":{{}}}}}}\n"
            );
            writer.write_all(call.as_bytes()).await.unwrap();
            writer.flush().await.unwrap();

            response.clear();
            reader.read_line(&mut response).await.unwrap();
            let response: Value = serde_json::from_str(&response).unwrap();
            assert_eq!(response["id"], id);
            assert_eq!(response["result"]["isError"], false);
            assert!(
                response["result"]["content"][0]["text"]
                    .as_str()
                    .unwrap()
                    .contains(expected)
            );
        }

        writer
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":6,\"method\":\"tools/call\",\"params\":{\"name\":\"get_current_frame\",\"arguments\":{}}}\n")
            .await
            .unwrap();
        writer.flush().await.unwrap();
        response.clear();
        reader.read_line(&mut response).await.unwrap();
        let response: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["id"], 6);
        assert_eq!(response["result"]["isError"], false);
        assert_eq!(response["result"]["content"][0]["type"], "image");
        assert_eq!(response["result"]["content"][0]["mimeType"], "image/png");

        drop(writer);
        drop(reader);
        server_task.await.unwrap();
        http_task.join().unwrap();
    }

    #[tokio::test]
    async fn stdio_write_tools_post_http_api_contracts() {
        let (endpoint, http_task) = serve_write_api();
        let (client_transport, server_transport) = tokio::io::duplex(32 * 1024);
        let server_task = tokio::spawn(async move {
            Aviutl2Mcp { endpoint }
                .serve(server_transport)
                .await
                .expect("server should initialize")
                .waiting()
                .await
                .expect("server should stop cleanly");
        });

        let (reader, mut writer) = tokio::io::split(client_transport);
        let mut reader = BufReader::new(reader);
        writer
            .write_all(concat!(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test-client","version":"1.0.0"}}}"#,
                "\n",
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                "\n"
            ).as_bytes())
            .await
            .unwrap();
        writer.flush().await.unwrap();

        let mut response = String::new();
        reader.read_line(&mut response).await.unwrap();
        assert_eq!(serde_json::from_str::<Value>(&response).unwrap()["id"], 1);

        let calls = [
            (
                2,
                "move_object",
                r#"{"expectedSceneName":"Root","objectId":"obj-1","destination":{"layer":2,"startFrame":100}}"#,
                r#""layer": 2"#,
            ),
            (
                3,
                "delete_object",
                r#"{"expectedSceneName":"Root","objectId":"obj-2"}"#,
                r#""deleted""#,
            ),
            (
                4,
                "create_text_object",
                r#"{"expectedSceneName":"Root","layer":1,"startFrame":100,"length":30,"text":"Hello"}"#,
                r#""text": "Hello""#,
            ),
            (
                5,
                "update_text_object",
                r#"{"expectedSceneName":"Root","objectId":"obj-text","patch":{"content":"Updated","font":null,"size":null,"position":null,"color":null}}"#,
                r#""content": "Updated""#,
            ),
            (
                6,
                "duplicate_object",
                r#"{"expectedSceneName":"Root","objectId":"obj-text","destination":{"layer":2,"startFrame":200}}"#,
                r#""startFrame": 200"#,
            ),
            (
                7,
                "create_media_object",
                r#"{"expectedSceneName":"Root","mediaPath":"C:\\media\\image.png","layer":3,"startFrame":300,"length":90}"#,
                r#""endFrame": 389"#,
            ),
        ];
        for (id, tool, arguments, expected) in calls {
            let arguments: Value = serde_json::from_str(arguments).unwrap();
            let call = serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": { "name": tool, "arguments": arguments },
            });
            writer.write_all(call.to_string().as_bytes()).await.unwrap();
            writer.write_all(b"\n").await.unwrap();
            writer.flush().await.unwrap();

            response.clear();
            reader.read_line(&mut response).await.unwrap();
            let response: Value = serde_json::from_str(&response).unwrap();
            assert_eq!(response["id"], id);
            assert_eq!(response["result"]["isError"], false);
            assert!(
                response["result"]["content"][0]["text"]
                    .as_str()
                    .unwrap()
                    .contains(expected)
            );
        }

        drop(writer);
        drop(reader);
        server_task.await.unwrap();
        http_task.join().unwrap();
    }
}
