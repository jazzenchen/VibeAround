//! MCP Streamable HTTP endpoint — POST /mcp
//!
//! Implements a JSON-RPC 2.0 server for the Model Context Protocol.
//! Methods: initialize, notifications/initialized, tools/list, tools/call.

use axum::{
    extract::State,
    response::IntoResponse,
    Json,
};

use super::AppState;

/// JSON-RPC 2.0 request envelope.
#[derive(serde::Deserialize)]
pub struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<serde_json::Value>,
    method: String,
    #[serde(default)]
    params: Option<serde_json::Value>,
}

/// Build a JSON-RPC 2.0 success response.
fn jsonrpc_ok(id: Option<serde_json::Value>, result: serde_json::Value) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    }))
}

/// Build a JSON-RPC 2.0 error response.
fn jsonrpc_err(id: Option<serde_json::Value>, code: i64, message: &str) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    }))
}

/// POST /mcp — MCP Streamable HTTP endpoint.
/// Handles JSON-RPC methods: initialize, notifications/initialized, tools/list, tools/call.
pub async fn mcp_handler(
    State(state): State<AppState>,
    Json(req): Json<JsonRpcRequest>,
) -> impl IntoResponse {
    if req.jsonrpc != "2.0" {
        return jsonrpc_err(req.id, -32600, "Invalid JSON-RPC version");
    }

    match req.method.as_str() {
        "initialize" => mcp_initialize(req.id),
        "notifications/initialized" => {
            // Client acknowledgement — no response needed, but Streamable HTTP expects one.
            jsonrpc_ok(req.id, serde_json::json!({}))
        }
        "tools/list" => mcp_tools_list(req.id),
        "tools/call" => mcp_tools_call(req.id, req.params, &state).await,
        _ => jsonrpc_err(req.id, -32601, &format!("Method not found: {}", req.method)),
    }
}

/// Handle "initialize" — return server info and capabilities.
fn mcp_initialize(id: Option<serde_json::Value>) -> Json<serde_json::Value> {
    jsonrpc_ok(id, serde_json::json!({
        "protocolVersion": "2025-03-26",
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name": "vibearound",
            "version": "0.1.0"
        }
    }))
}

/// Handle "tools/list" — return the dispatch_task tool schema.
fn mcp_tools_list(id: Option<serde_json::Value>) -> Json<serde_json::Value> {
    jsonrpc_ok(id, serde_json::json!({
        "tools": [{
            "name": "dispatch_task",
            "description": "Dispatch a task to a worker agent on a project workspace. If no worker is running on the workspace, one will be auto-spawned.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "workspace": {
                        "type": "string",
                        "description": "Absolute path to the project workspace directory (e.g. ~/.vibearound/workspaces/my-project/). Must be a project-specific directory, NOT the root ~/.vibearound/ directory. Create the directory first if it does not exist."
                    },
                    "message": {
                        "type": "string",
                        "description": "The task or question for the worker agent"
                    },
                    "kind": {
                        "type": "string",
                        "description": "Agent type: claude, gemini, opencode, or codex. If omitted, uses the default agent.",
                        "enum": ["claude", "gemini", "opencode", "codex"]
                    }
                },
                "required": ["workspace", "message"]
            }
        }]
    }))
}

/// Handle "tools/call" — dispatch task to worker.
async fn mcp_tools_call(
    id: Option<serde_json::Value>,
    params: Option<serde_json::Value>,
    _state: &AppState,
) -> Json<serde_json::Value> {
    let params = match params {
        Some(p) => p,
        None => return jsonrpc_err(id, -32602, "Missing params"),
    };

    let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
    if tool_name != "dispatch_task" {
        return jsonrpc_err(id, -32602, &format!("Unknown tool: {}", tool_name));
    }

    let arguments = match params.get("arguments") {
        Some(a) => a,
        None => return jsonrpc_err(id, -32602, "Missing arguments"),
    };

    let workspace = match arguments.get("workspace").and_then(|v| v.as_str()) {
        Some(w) => std::path::PathBuf::from(w),
        None => return jsonrpc_err(id, -32602, "Missing required argument: workspace"),
    };

    // Guard: reject if workspace is the vibearound root directory
    let data_dir = common::config::data_dir();
    if workspace == data_dir || workspace == data_dir.join("") {
        return jsonrpc_ok(id, serde_json::json!({
            "content": [{
                "type": "text",
                "text": format!(
                    "Error: workspace must be a project-specific directory under {}/workspaces/<project-name>/, \
                     not the root data directory. Please create the workspace directory first.",
                    data_dir.display()
                )
            }],
            "isError": true
        }));
    }
    let message = match arguments.get("message").and_then(|v| v.as_str()) {
        Some(m) => m,
        None => return jsonrpc_err(id, -32602, "Missing required argument: message"),
    };

    // Inject current date so the worker knows what "today" is
    let date_str = chrono::Local::now().format("%Y-%m-%d").to_string();
    let _message_with_date = format!("[Current date: {}]\n\n{}", date_str, message);
    let _kind = arguments
        .get("kind")
        .and_then(|v| v.as_str())
        .and_then(common::agent::AgentKind::from_str_loose);

    // TODO: migrate to AgentHub
    jsonrpc_ok(id, serde_json::json!({
        "content": [{
            "type": "text",
            "text": "MCP dispatch_task is not yet available in the new hub architecture"
        }],
        "isError": true
    }))
}
