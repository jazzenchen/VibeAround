//! MCP Streamable HTTP endpoint — POST/GET /mcp
//!
//! Implements a JSON-RPC 2.0 server for the Model Context Protocol.
//! Methods: initialize, notifications/initialized, tools/list, tools/call.
//! Optional resource/prompt list methods return empty lists so clients that
//! probe the full MCP surface do not treat VibeAround as disconnected.
//!
//! Most MCP tools are stateless — they validate inputs and return text.
//! Collaboration tools are the exception: `va_mcp_initialize_subagents` creates
//! git worktrees and records the resulting multi-agent turn on a workspace
//! thread, but still does not drive live agent processes directly.
//!
//! ## Module layout
//!
//! - [`jsonrpc`] — JSON-RPC 2.0 envelope + MCP content helpers
//! - [`tools`]   — session, file, handover, and workspace tool implementations
//! - [`subagents`] — multi-agent tool handlers and runtime notifications
//! - [`subagent_worktrees`] — git worktree setup and cleanup
//! - [`sessions`] — per-agent on-disk session auto-discovery

mod jsonrpc;
mod preview;
mod preview_conversation;
mod session_identity;
mod sessions;
mod subagent_worktrees;
mod subagents;
mod tools;

use axum::{
    extract::State,
    http::{HeaderName, HeaderValue, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    Json,
};
use futures_util::stream;
use std::{convert::Infallible, time::Duration};

use super::AppState;

use jsonrpc::{jsonrpc_err, jsonrpc_ok, JsonRpcRequest};

const MCP_SESSION_ID_HEADER: HeaderName = HeaderName::from_static("mcp-session-id");
const MCP_SSE_KEEPALIVE_SECS: u64 = 15;

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
        "initialize" => mcp_initialize(req.id),
        "tools/list" => mcp_tools_list(req.id).into_response(),
        "resources/list" => mcp_resources_list(req.id).into_response(),
        "resources/templates/list" => mcp_resource_templates_list(req.id).into_response(),
        "prompts/list" => mcp_prompts_list(req.id).into_response(),
        "tools/call" => mcp_tools_call(req.id, req.params, &state)
            .await
            .into_response(),
        _ => jsonrpc_err(req.id, -32601, &format!("Method not found: {}", req.method))
            .into_response(),
    }
}

/// GET /mcp — open a server-to-client SSE stream.
///
/// VibeAround currently has no server-initiated MCP notifications to send, but
/// Streamable HTTP clients such as Claude Code still expect this connection to
/// establish successfully for reconnect support.
pub async fn mcp_sse_handler() -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    Sse::new(stream::pending::<Result<Event, Infallible>>()).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(MCP_SSE_KEEPALIVE_SECS))
            .text("keepalive"),
    )
}

fn mcp_initialize(id: Option<serde_json::Value>) -> axum::response::Response {
    let mut response = jsonrpc_ok(
        id,
        serde_json::json!({
            "protocolVersion": "2025-03-26",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "vibearound", "version": env!("CARGO_PKG_VERSION") }
        }),
    )
    .into_response();
    let session_id = uuid::Uuid::new_v4().to_string();
    response.headers_mut().insert(
        MCP_SESSION_ID_HEADER,
        HeaderValue::from_str(&session_id).expect("UUID is a valid HTTP header value"),
    );
    response
}

fn mcp_tools_list(id: Option<serde_json::Value>) -> Json<serde_json::Value> {
    jsonrpc_ok(id, common::resources::mcp_tools_list_json())
}

fn mcp_resources_list(id: Option<serde_json::Value>) -> Json<serde_json::Value> {
    jsonrpc_ok(id, serde_json::json!({ "resources": [] }))
}

fn mcp_resource_templates_list(id: Option<serde_json::Value>) -> Json<serde_json::Value> {
    jsonrpc_ok(id, serde_json::json!({ "resourceTemplates": [] }))
}

fn mcp_prompts_list(id: Option<serde_json::Value>) -> Json<serde_json::Value> {
    jsonrpc_ok(id, serde_json::json!({ "prompts": [] }))
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
        "va_mcp_get_session_id" => {
            tools::mcp_get_session_id(id, arguments, params.get("_meta"), state).await
        }
        "va_mcp_send_file" => tools::mcp_send_file(id, arguments, state).await,
        "va_mcp_prepare_handover" => tools::mcp_prepare_handover(id, arguments).await,
        "va_mcp_register_workspace" => tools::mcp_register_workspace(id, arguments).await,
        "va_mcp_initialize_subagents" => subagents::mcp_initialize_subagents(id, arguments, state).await,
        "va_mcp_wait_for_subagents" => subagents::mcp_wait_for_subagents(id, arguments, state).await,
        "va_mcp_preview" => preview::mcp_preview(id, arguments, params.get("_meta"), state).await,
        _ => jsonrpc_err(id, -32602, &format!("Unknown tool: {}", tool_name)),
    }
}

#[cfg(test)]
mod tests {
    use axum::{http::header, response::IntoResponse};
    use serde_json::json;

    use super::MCP_SESSION_ID_HEADER;

    #[test]
    fn initialize_returns_mcp_session_id_header() {
        let response = super::mcp_initialize(Some(json!(1)));
        assert_eq!(response.status(), axum::http::StatusCode::OK);

        let session_id = response
            .headers()
            .get(MCP_SESSION_ID_HEADER)
            .and_then(|value| value.to_str().ok())
            .expect("initialize response includes Mcp-Session-Id");
        assert!(uuid::Uuid::parse_str(session_id).is_ok());
    }

    #[tokio::test]
    async fn get_mcp_returns_sse_stream() {
        let response = super::mcp_sse_handler().await.into_response();
        assert_eq!(response.status(), axum::http::StatusCode::OK);

        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .expect("SSE response has content type");
        assert!(content_type.starts_with("text/event-stream"));
    }

    #[test]
    fn optional_mcp_lists_return_empty_successes() {
        assert_eq!(
            super::mcp_resources_list(Some(json!(1))).0,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": { "resources": [] }
            })
        );
        assert_eq!(
            super::mcp_resource_templates_list(Some(json!(2))).0,
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "result": { "resourceTemplates": [] }
            })
        );
        assert_eq!(
            super::mcp_prompts_list(Some(json!(3))).0,
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "result": { "prompts": [] }
            })
        );
    }

    #[test]
    fn preview_exposes_one_tool_with_exactly_one_runtime_source() {
        let response = super::mcp_tools_list(Some(json!(4))).0;
        let tools = response["result"]["tools"].as_array().unwrap();
        assert!(tools.iter().all(|tool| tool["name"] != "md_preview"));

        let preview = tools
            .iter()
            .find(|tool| tool["name"] == "va_mcp_preview")
            .expect("preview tool");
        let properties = preview["inputSchema"]["properties"].as_object().unwrap();
        assert!(properties.contains_key("port"));
        assert!(properties.contains_key("file"));
        assert_eq!(preview["inputSchema"]["required"], json!(["cwd"]));
    }
}
