pub mod handlers;
pub mod protocol;

use std::io::{BufRead, Write};

use serde_json::Value;

use protocol::{
    InitializeResult, JsonRpcRequest, JsonRpcResponse, ServerCapabilities, ServerInfo,
    ToolCallParams, ToolsCapability, ToolsListResult,
};

/// Run the MCP server over stdin/stdout.
///
/// Reads JSON-RPC messages (one per line) from stdin and writes responses to stdout.
/// Supports the MCP lifecycle: initialize, notifications/initialized, tools/list, tools/call.
pub fn run_server() -> Result<(), String> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();

    eprintln!("tomegane MCP server started");

    for line in stdin.lock().lines() {
        let line = line.map_err(|e| format!("Failed to read stdin: {e}"))?;
        let line = line.trim().to_string();

        if line.is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let response = JsonRpcResponse::error(None, -32700, format!("Parse error: {e}"));
                write_response(&mut stdout, &response)?;
                continue;
            }
        };

        let response = handle_request(&request);

        // Notifications (no id) don't get responses
        if request.id.is_none() {
            continue;
        }

        if let Some(response) = response {
            write_response(&mut stdout, &response)?;
        }
    }

    Ok(())
}

fn write_response(stdout: &mut impl Write, response: &JsonRpcResponse) -> Result<(), String> {
    let json = serde_json::to_string(response)
        .map_err(|e| format!("Failed to serialize response: {e}"))?;
    writeln!(stdout, "{json}").map_err(|e| format!("Failed to write to stdout: {e}"))?;
    stdout
        .flush()
        .map_err(|e| format!("Failed to flush stdout: {e}"))?;
    Ok(())
}

fn handle_request(request: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    match request.method.as_str() {
        "initialize" => {
            let result = InitializeResult {
                protocol_version: "2024-11-05".to_string(),
                capabilities: ServerCapabilities {
                    tools: ToolsCapability {},
                },
                server_info: ServerInfo {
                    name: "tomegane".to_string(),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                },
            };

            Some(JsonRpcResponse::success(
                request.id.clone(),
                serde_json::to_value(result).unwrap(),
            ))
        }

        "notifications/initialized" => {
            // Client acknowledges initialization — no response needed
            None
        }

        "tools/list" => {
            let result = ToolsListResult {
                tools: handlers::tool_definitions(),
            };

            Some(JsonRpcResponse::success(
                request.id.clone(),
                serde_json::to_value(result).unwrap(),
            ))
        }

        "tools/call" => {
            let params: ToolCallParams = match &request.params {
                Some(p) => match serde_json::from_value(p.clone()) {
                    Ok(tc) => tc,
                    Err(e) => {
                        return Some(JsonRpcResponse::error(
                            request.id.clone(),
                            -32602,
                            format!("Invalid params: {e}"),
                        ));
                    }
                },
                None => {
                    return Some(JsonRpcResponse::error(
                        request.id.clone(),
                        -32602,
                        "Missing params".to_string(),
                    ));
                }
            };

            let arguments = params
                .arguments
                .unwrap_or(Value::Object(Default::default()));
            let result = handlers::handle_tool_call(&params.name, &arguments);

            Some(JsonRpcResponse::success(
                request.id.clone(),
                serde_json::to_value(result).unwrap(),
            ))
        }

        _ => Some(JsonRpcResponse::error(
            request.id.clone(),
            -32601,
            format!("Method not found: {}", request.method),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn handle_initialize_returns_server_info() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "initialize".to_string(),
            params: None,
        };

        let response = handle_request(&request).unwrap();
        let result = response.result.unwrap();
        assert_eq!(result["serverInfo"]["name"], "tomegane");
        assert_eq!(result["protocolVersion"], "2024-11-05");
    }

    #[test]
    fn handle_tools_list_returns_three_tools() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(2)),
            method: "tools/list".to_string(),
            params: None,
        };

        let response = handle_request(&request).unwrap();
        let result = response.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 3);

        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"analyze_video"));
        assert!(names.contains(&"get_frame"));
        assert!(names.contains(&"compare_frames"));
    }

    #[test]
    fn handle_tools_call_with_missing_video() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(3)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "analyze_video",
                "arguments": {
                    "video_path": "/nonexistent/video.mp4"
                }
            })),
        };

        let response = handle_request(&request).unwrap();
        let result = response.result.unwrap();
        assert_eq!(result["isError"], true);
    }

    #[test]
    fn handle_unknown_method_returns_error() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(4)),
            method: "unknown/method".to_string(),
            params: None,
        };

        let response = handle_request(&request).unwrap();
        assert!(response.error.is_some());
        assert_eq!(response.error.unwrap().code, -32601);
    }

    #[test]
    fn notifications_return_no_response() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: "notifications/initialized".to_string(),
            params: None,
        };

        let response = handle_request(&request);
        assert!(response.is_none());
    }

    #[test]
    fn handle_tools_call_analyze_with_fixture() {
        let fixture =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/test_video.mp4");

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(5)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "analyze_video",
                "arguments": {
                    "video_path": fixture.to_string_lossy(),
                    "max_frames": 3
                }
            })),
        };

        let response = handle_request(&request).unwrap();
        let result = response.result.unwrap();
        assert!(result["isError"].is_null(), "Should not be an error");

        let content = result["content"].as_array().unwrap();
        // Should have text blocks and image blocks
        assert!(!content.is_empty());
        assert_eq!(content[0]["type"], "text");
    }

    #[test]
    fn handle_get_frame_with_fixture() {
        let fixture =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/test_video.mp4");

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(6)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "get_frame",
                "arguments": {
                    "video_path": fixture.to_string_lossy(),
                    "timestamp_seconds": 2.0
                }
            })),
        };

        let response = handle_request(&request).unwrap();
        let result = response.result.unwrap();
        assert!(result["isError"].is_null());

        let content = result["content"].as_array().unwrap();
        // Should have a text block and an image block
        assert!(content.len() >= 2);
        assert_eq!(content[1]["type"], "image");
    }
}
