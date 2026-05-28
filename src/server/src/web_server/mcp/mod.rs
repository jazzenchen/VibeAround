//! MCP Streamable HTTP endpoint — POST /mcp
//!
//! Implements a JSON-RPC 2.0 server for the Model Context Protocol.
//! Methods: initialize, notifications/initialized, tools/list, tools/call.
//!
//! Most MCP tools are stateless — they validate inputs and return text.
//! Collaboration tools are the exception: `initialize_subagents` creates
//! git worktrees and records the resulting multi-agent turn on a workspace
//! thread, but still does not drive live agent processes directly.
//!
//! ## Module layout
//!
//! - [`jsonrpc`] — JSON-RPC 2.0 envelope + MCP content helpers
//! - [`tools`]   — the five `tools/call` implementations
//! - [`sessions`] — per-agent on-disk session auto-discovery
//! - [`ports`]   — deny-list of well-known service ports

mod jsonrpc;
mod ports;
mod sessions;
mod tools;

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};

use super::AppState;

use jsonrpc::{jsonrpc_err, jsonrpc_ok, JsonRpcRequest};

/// POST /mcp — MCP Streamable HTTP endpoint.
pub async fn mcp_handler(
    State(state): State<AppState>,
    Json(req): Json<JsonRpcRequest>,
) -> axum::response::Response {
    if req.jsonrpc != "2.0" {
        return jsonrpc_err(req.id, -32600, "Invalid JSON-RPC version").into_response();
    }

    // Notifications (no id) must return 202 Accepted with no body per MCP spec.
    if req.method.starts_with("notifications/") {
        return StatusCode::ACCEPTED.into_response();
    }

    match req.method.as_str() {
        "initialize" => mcp_initialize(req.id).into_response(),
        "tools/list" => mcp_tools_list(req.id).into_response(),
        "tools/call" => mcp_tools_call(req.id, req.params, &state)
            .await
            .into_response(),
        _ => jsonrpc_err(req.id, -32601, &format!("Method not found: {}", req.method))
            .into_response(),
    }
}

fn mcp_initialize(id: Option<serde_json::Value>) -> Json<serde_json::Value> {
    jsonrpc_ok(
        id,
        serde_json::json!({
            "protocolVersion": "2025-03-26",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "vibearound", "version": env!("CARGO_PKG_VERSION") }
        }),
    )
}

fn mcp_tools_list(id: Option<serde_json::Value>) -> Json<serde_json::Value> {
    jsonrpc_ok(id, common::resources::mcp_tools_list_json())
}

async fn mcp_tools_call(
    id: Option<serde_json::Value>,
    params: Option<serde_json::Value>,
    state: &AppState,
) -> Json<serde_json::Value> {
    let params = match params {
        Some(p) => p,
        None => return jsonrpc_err(id, -32602, "Missing params"),
    };

    let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let arguments = match params.get("arguments") {
        Some(a) => a,
        None => return jsonrpc_err(id, -32602, "Missing arguments"),
    };

    match tool_name {
        "get_session_id" => tools::mcp_get_session_id(id, arguments, state).await,
        "prepare_handover" => tools::mcp_prepare_handover(id, arguments).await,
        "register_workspace" => tools::mcp_register_workspace(id, arguments).await,
        "initialize_subagents" => tools::mcp_initialize_subagents(id, arguments, state).await,
        "wait_for_subagents" => tools::mcp_wait_for_subagents(id, arguments, state).await,
        "preview" => tools::mcp_preview_start(id, arguments, state).await,
        "md_preview" => tools::mcp_md_preview(id, arguments, state).await,
        _ => jsonrpc_err(id, -32602, &format!("Unknown tool: {}", tool_name)),
    }
}
