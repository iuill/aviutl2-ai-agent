use std::io::{self, BufRead, Write};

use anyhow::{Context, Result};
use aviutl2_ai_agent_protocol::{ApiError, CurrentObjects, CurrentScene, CurrentTimeline};
use clap::Parser;
use serde_json::{Value, json};

#[derive(Debug, Parser)]
#[command(version, about = "Read-only MCP server for AviUtl2")]
struct Args {
    #[arg(long, default_value = "http://127.0.0.1:7890")]
    endpoint: String,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.context("failed to read MCP request")?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = serde_json::from_str(&line).context("invalid MCP JSON")?;
        if let Some(response) = dispatch(&args.endpoint, &request) {
            serde_json::to_writer(&mut stdout, &response)?;
            writeln!(stdout)?;
            stdout.flush()?;
        }
    }
    Ok(())
}

fn dispatch(endpoint: &str, request: &Value) -> Option<Value> {
    let id = request.get("id")?.clone();
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": "2025-06-18",
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": "aviutl2-ai-agent",
                "version": env!("CARGO_PKG_VERSION")
            }
        })),
        "tools/list" => Ok(json!({
            "tools": [
                {
                    "name": "get_current_scene",
                    "description": "現在選択されているAviUtl2 sceneを読み取ります。",
                    "inputSchema": {
                        "type": "object",
                        "properties": {},
                        "additionalProperties": false
                    }
                },
                {
                    "name": "get_current_timeline",
                    "description": "現在のsceneのtimeline概要を読み取ります。",
                    "inputSchema": {
                        "type": "object",
                        "properties": {},
                        "additionalProperties": false
                    }
                },
                {
                    "name": "list_current_objects",
                    "description": "現在のsceneにあるobjectのsnapshotを読み取ります。",
                    "inputSchema": {
                        "type": "object",
                        "properties": {},
                        "additionalProperties": false
                    }
                }
            ]
        })),
        "tools/call" => call_tool(endpoint, request.get("params").unwrap_or(&Value::Null)),
        _ => return Some(error(id, -32601, "Method not found")),
    };
    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(message) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "content": [{ "type": "text", "text": message }],
                "isError": true
            }
        }),
    })
}

fn call_tool(endpoint: &str, params: &Value) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "tool name is required".to_owned())?;
    let arguments = params.get("arguments").unwrap_or(&Value::Null);
    if arguments.as_object().is_none_or(|value| !value.is_empty()) {
        return Err("tool arguments must be an empty object".to_owned());
    }
    let value = match name {
        "get_current_scene" => get::<CurrentScene>(endpoint, "/v1/scenes/current"),
        "get_current_timeline" => get::<CurrentTimeline>(endpoint, "/v1/scenes/current/timeline"),
        "list_current_objects" => get::<CurrentObjects>(endpoint, "/v1/scenes/current/objects"),
        _ => return Err(format!("unknown tool: {name}")),
    }?;
    let text = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
    Ok(json!({ "content": [{ "type": "text", "text": text }] }))
}

fn get<T: serde::de::DeserializeOwned + serde::Serialize>(
    base_endpoint: &str,
    path: &str,
) -> Result<Value, String> {
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
    serde_json::to_value(value).map_err(|error| error.to_string())
}

fn error(id: Value, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

#[cfg(test)]
mod tests {
    use super::dispatch;
    use serde_json::json;

    #[test]
    fn initialize_advertises_tools_capability() {
        let response = dispatch(
            "http://127.0.0.1:1",
            &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        )
        .unwrap();
        assert_eq!(response["result"]["protocolVersion"], "2025-06-18");
        assert!(response["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn tools_are_read_only_and_take_no_arguments() {
        let response = dispatch(
            "http://127.0.0.1:1",
            &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        )
        .unwrap();
        let tools = response["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 3);
        assert_eq!(tools[0]["name"], "get_current_scene");
        assert_eq!(tools[1]["name"], "get_current_timeline");
        assert_eq!(tools[2]["name"], "list_current_objects");
        assert!(
            tools
                .iter()
                .all(|tool| tool["inputSchema"]["additionalProperties"] == false)
        );
    }

    #[test]
    fn notifications_do_not_receive_a_response() {
        assert!(
            dispatch(
                "http://127.0.0.1:1",
                &json!({"jsonrpc":"2.0","method":"notifications/initialized"})
            )
            .is_none()
        );
    }
}
