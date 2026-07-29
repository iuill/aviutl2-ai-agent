use anyhow::Result;
use aviutl2_ai_agent_protocol::{ApiError, CurrentObjects, CurrentScene, CurrentTimeline};
use clap::Parser;
use rmcp::{
    ErrorData as McpError, ServiceExt,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock},
    schemars, tool, tool_router,
    transport::stdio,
};
use serde::de::DeserializeOwned;

#[derive(Debug, Parser)]
#[command(version, about = "Read-only MCP server for AviUtl2")]
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

#[tool_router(server_handler)]
impl Aviutl2Mcp {
    #[tool(description = "現在選択されているAviUtl2 sceneを読み取ります。")]
    fn get_current_scene(
        &self,
        Parameters(_): Parameters<EmptyParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(self.get::<CurrentScene>("/v1/scenes/current"))
    }

    #[tool(description = "現在のsceneのtimeline概要を読み取ります。")]
    fn get_current_timeline(
        &self,
        Parameters(_): Parameters<EmptyParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(self.get::<CurrentTimeline>("/v1/scenes/current/timeline"))
    }

    #[tool(description = "現在のsceneにあるobjectのsnapshotを読み取ります。")]
    fn list_current_objects(
        &self,
        Parameters(_): Parameters<EmptyParams>,
    ) -> Result<CallToolResult, McpError> {
        Ok(self.get::<CurrentObjects>("/v1/scenes/current/objects"))
    }
}

impl Aviutl2Mcp {
    fn get<T: DeserializeOwned + serde::Serialize>(&self, path: &str) -> CallToolResult {
        match get::<T>(&self.endpoint, path) {
            Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
            Err(message) => CallToolResult::error(vec![ContentBlock::text(message)]),
        }
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
    if !response.status().is_success() {
        let error = response
            .body_mut()
            .read_json::<ApiError>()
            .map_err(|error| error.to_string())?;
        return Err(format!("AviUtl2 API error: {}", error.message));
    }
    let value: T = response
        .body_mut()
        .read_json()
        .map_err(|error| error.to_string())?;
    serde_json::to_string_pretty(&value).map_err(|error| error.to_string())
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
    use std::borrow::Cow;

    use rmcp::{
        ServerHandler, ServiceExt,
        model::{ProtocolVersion, ServerCapabilities},
    };
    use serde_json::Value;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    use super::Aviutl2Mcp;

    fn server() -> Aviutl2Mcp {
        Aviutl2Mcp {
            endpoint: "http://127.0.0.1:1".to_owned(),
        }
    }

    #[test]
    fn tools_are_read_only_and_take_no_arguments() {
        let tools = Aviutl2Mcp::tool_router().list_all();
        assert_eq!(tools.len(), 3);
        assert_eq!(tools[0].name, "get_current_scene");
        assert_eq!(tools[1].name, "get_current_timeline");
        assert_eq!(tools[2].name, "list_current_objects");
        assert!(tools.iter().all(|tool| {
            tool.input_schema
                .get("properties")
                .is_none_or(|value| value.as_object().is_some_and(serde_json::Map::is_empty))
                && tool.input_schema.get("additionalProperties")
                    == Some(&serde_json::Value::Bool(false))
        }));
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
}
