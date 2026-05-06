//! MCP `tools/call` implementations.
//!
//! Each tool is a pure function that takes the JSON-RPC id + arguments,
//! validates inputs, touches workspace config / preview store / session
//! files, and returns a JSON-RPC response.
//!
//! Tools never touch `ConversationManager`, pods, or bridges — they're stateless. Any
//! session loading happens later when the user sends `/pickup` in IM chat.

use axum::Json;

use crate::web_server::AppState;

use super::jsonrpc::{jsonrpc_err, mcp_error_text, mcp_text};
use super::ports::is_denied_port;
use super::sessions::find_latest_session;

// ---------------------------------------------------------------------------
// get_session_id — resolve the current ACP session ID from route info
// ---------------------------------------------------------------------------

pub(super) async fn mcp_get_session_id(
    id: Option<serde_json::Value>,
    arguments: &serde_json::Value,
    state: &AppState,
) -> Json<serde_json::Value> {
    let channel_kind = match arguments.get("channel_kind").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return jsonrpc_err(id, -32602, "Missing required argument: channel_kind"),
    };
    let chat_id = match arguments.get("chat_id").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return jsonrpc_err(id, -32602, "Missing required argument: chat_id"),
    };

    let route = common::routing::RouteKey::new(channel_kind, chat_id);
    let conversation_manager = state.channel_hub.conversation_manager();

    let state_opt = match conversation_manager.conversation(&route) {
        Some(conv) => Some(conv.state().await),
        None => None,
    };
    match state_opt {
        Some(snap) if snap.session_id.is_some() => {
            let sid = snap.session_id.unwrap();
            mcp_text(id, &sid)
        }
        _ => mcp_error_text(
            id,
            "No active session found for this route. The agent session may not have started yet.",
        ),
    }
}

// ---------------------------------------------------------------------------
// prepare_handover — stateless, no ConversationManager dependency
// ---------------------------------------------------------------------------

pub(super) async fn mcp_prepare_handover(
    id: Option<serde_json::Value>,
    arguments: &serde_json::Value,
) -> Json<serde_json::Value> {
    let cwd = match arguments.get("cwd").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return jsonrpc_err(id, -32602, "Missing required argument: cwd"),
    };
    let session_id_arg = arguments
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(String::from);
    let agent_kind = match arguments.get("agent_kind").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => return jsonrpc_err(id, -32602, "Missing required argument: agent_kind"),
    };
    let agent_kind_str = agent_kind;

    // Validate cwd is a known workspace.
    // Built-in workspaces under ~/.vibearound/workspaces/ are always accepted.
    let config = common::config::ensure_loaded();
    let cwd_path = std::path::PathBuf::from(cwd);
    let builtin_dir = common::config::builtin_workspaces_dir();
    let is_builtin = cwd_path.starts_with(&builtin_dir);
    let is_registered = config.all_workspaces().iter().any(|ws| ws == &cwd_path);

    if !is_builtin && !is_registered {
        return mcp_error_text(
            id,
            &format!(
                "Workspace {} is not registered in VibeAround.\n\
             Use the `register_workspace` tool to add it first, then retry.",
                cwd
            ),
        );
    }

    // Resolve session ID: use provided value, or auto-discover from session files
    let session_id = match session_id_arg {
        Some(sid) if !sid.is_empty() => sid,
        _ => match find_latest_session(agent_kind_str, &cwd_path) {
            Some(sid) => sid,
            None => {
                let hint = match agent_kind_str {
                    "claude" => "In Claude Code, you can find it by running /status.",
                    "gemini" => "In Gemini CLI, run /resume to browse recent sessions.",
                    "codex" => "In Codex CLI, run `codex resume` to see recent sessions.",
                    "opencode" => "In opencode, run `opencode session list` to see recent sessions.",
                    _ => "Check your agent's session history.",
                };
                return mcp_error_text(
                    id,
                    &format!(
                        "Could not auto-discover session ID. Please provide your session_id explicitly.\n{}",
                        hint
                    ),
                );
            }
        },
    };

    let code = common::conversations::handover::pickup_codes::store(
        agent_kind_str.to_string(),
        session_id,
        cwd.to_string(),
    );
    let pickup_cmd = format!("/pickup {}", code);
    mcp_text(
        id,
        &format!(
            "Handover prepared.\n\n\
         Tell the user to send this command in any IM chat connected to VibeAround:\n\
         {}\n\n\
         The code expires in 2 minutes. After sending the command, the user's next message will resume this session.",
            pickup_cmd
        ),
    )
}

// ---------------------------------------------------------------------------
// register_workspace — writes to VibeAround settings.json
// ---------------------------------------------------------------------------

pub(super) async fn mcp_register_workspace(
    id: Option<serde_json::Value>,
    arguments: &serde_json::Value,
) -> Json<serde_json::Value> {
    let cwd = match arguments.get("cwd").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return jsonrpc_err(id, -32602, "Missing required argument: cwd"),
    };

    let cwd_path = std::path::PathBuf::from(cwd);
    if !cwd_path.is_dir() {
        return mcp_error_text(id, &format!("Directory does not exist: {}", cwd));
    }

    // Check if already registered
    let config = common::config::ensure_loaded();
    let already_registered = config.all_workspaces().iter().any(|ws| ws == &cwd_path);

    if already_registered {
        return mcp_text(id, &format!("Workspace {} is already registered.", cwd));
    }

    // Add to settings.json
    let cwd_owned = cwd.to_string();
    if let Err(e) = common::config::update_settings_json(move |settings| {
        if let Some(obj) = settings.as_object_mut() {
            let workspaces = obj
                .entry("workspaces")
                .or_insert_with(|| serde_json::json!([]));
            if let Some(arr) = workspaces.as_array_mut() {
                arr.push(serde_json::Value::String(cwd_owned));
            }
        }
    }) {
        return mcp_error_text(id, &format!("Failed to update settings: {}", e));
    }

    mcp_text(id, &format!("Workspace {} registered successfully.", cwd))
}

// ---------------------------------------------------------------------------
// preview_start — register a live preview for a running local server
// ---------------------------------------------------------------------------

pub(super) async fn mcp_preview_start(
    id: Option<serde_json::Value>,
    arguments: &serde_json::Value,
    state: &AppState,
) -> Json<serde_json::Value> {
    let port = match arguments.get("port").and_then(|v| v.as_u64()) {
        Some(p) if p > 0 && p <= 65535 => p as u16,
        _ => {
            return jsonrpc_err(
                id,
                -32602,
                "Missing or invalid required argument: port (1-65535)",
            )
        }
    };

    if is_denied_port(port) {
        return mcp_error_text(
            id,
            &format!(
                "Port {} is a well-known service port and cannot be previewed for security reasons. \
             Use a typical dev server port (e.g. 3000, 5173, 8080).",
                port
            ),
        );
    }

    let cwd = match arguments.get("cwd").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return jsonrpc_err(id, -32602, "Missing required argument: cwd"),
    };

    let cwd_path = std::path::PathBuf::from(cwd);
    if let Err(resp) = validate_workspace(&cwd_path, id.clone()) {
        return resp;
    }

    let title = derive_title(arguments, &cwd_path);
    let session_id = arguments
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(String::from);

    let (owner_slug, share_slug) =
        common::previews::ensure_server(port, cwd_path, title, session_id.clone());
    let owner_url = build_preview_url(state, "preview/u", &owner_slug);
    let share_url = build_preview_url(state, "preview/s", &share_slug);

    let session_hint = if session_id.is_none() {
        "\n\n\u{26a0}\u{fe0f} No session_id provided. Use /va-session skill to resolve it and pass session_id for automatic dev-server cleanup."
    } else {
        ""
    };

    mcp_text(
        id,
        &format!(
            "Preview ready.\n\n\
         Owner: `{}`\n\
         Share: `{}`\n\
         Port: {}\n\
         Share expires: 10 minutes{}",
            owner_url, share_url, port, session_hint
        ),
    )
}

// ---------------------------------------------------------------------------
// md_preview — render a markdown file with styled preview
// ---------------------------------------------------------------------------

pub(super) async fn mcp_md_preview(
    id: Option<serde_json::Value>,
    arguments: &serde_json::Value,
    state: &AppState,
) -> Json<serde_json::Value> {
    let file_str = match arguments.get("file").and_then(|v| v.as_str()) {
        Some(f) => f,
        None => return jsonrpc_err(id, -32602, "Missing required argument: file"),
    };
    let cwd = match arguments.get("cwd").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return jsonrpc_err(id, -32602, "Missing required argument: cwd"),
    };

    let cwd_path = std::path::PathBuf::from(cwd);
    if let Err(resp) = validate_workspace(&cwd_path, id.clone()) {
        return resp;
    }

    // Resolve relative paths against cwd.
    let file_path = {
        let p = std::path::PathBuf::from(file_str);
        if p.is_relative() {
            cwd_path.join(&p)
        } else {
            p
        }
    };
    if !file_path.is_file() {
        return mcp_error_text(id, &format!("File not found: {}", file_path.display()));
    }

    // Security: file must be inside the workspace.
    if let (Ok(canon_file), Ok(canon_ws)) = (file_path.canonicalize(), cwd_path.canonicalize()) {
        if !canon_file.starts_with(&canon_ws) {
            return mcp_error_text(id, "File must be inside the workspace directory.");
        }
    }

    let title = arguments
        .get("title")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| {
            file_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Preview")
                .to_string()
        });

    let (owner_slug, share_slug) = common::previews::ensure_file(file_path, cwd_path, title);
    let owner_url = build_preview_url(state, "preview/u", &owner_slug);
    let share_url = build_preview_url(state, "preview/s", &share_slug);

    mcp_text(
        id,
        &format!(
            "Markdown preview ready.\n\n\
         Owner: `{}`\n\
         Share: `{}`\n\
         Share expires: 10 minutes",
            owner_url, share_url
        ),
    )
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Validate that cwd is a registered workspace. Returns Err with a JSON-RPC
/// error response on failure.
fn validate_workspace(
    cwd_path: &std::path::Path,
    id: Option<serde_json::Value>,
) -> Result<(), Json<serde_json::Value>> {
    let config = common::config::ensure_loaded();
    let builtin_dir = common::config::builtin_workspaces_dir();
    let is_builtin = cwd_path.starts_with(&builtin_dir);
    let is_registered = config.all_workspaces().iter().any(|ws| ws == cwd_path);

    if !is_builtin && !is_registered {
        return Err(mcp_error_text(
            id,
            &format!(
                "Workspace {} is not registered in VibeAround.\n\
             Use the `register_workspace` tool to add it first, then retry.",
                cwd_path.display()
            ),
        ));
    }
    Ok(())
}

/// Derive a title from the MCP arguments or the workspace directory name.
fn derive_title(arguments: &serde_json::Value, cwd_path: &std::path::Path) -> String {
    arguments
        .get("title")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| {
            cwd_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Preview")
                .to_string()
        })
}

/// Build a full preview URL from the tunnel (or localhost fallback).
/// All preview routes live under `/va/` to avoid conflicts with dev servers.
fn build_preview_url(state: &AppState, route: &str, slug: &str) -> String {
    let base = state
        .tunnels
        .first_url()
        .unwrap_or_else(|| format!("http://127.0.0.1:{}", state.port));
    format!("{}/va/{}/{}", base.trim_end_matches('/'), route, slug)
}
