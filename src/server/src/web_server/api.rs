//! REST API handlers for the web server.
//!
//! - GET /api/sessions
//! - POST /api/sessions
//! - DELETE /api/sessions/:session_id
//! - GET /api/tmux/sessions
//! - GET /api/agents
//! - GET /api/services
//! - DELETE /api/services/:category/:id

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};

use common::config;
use common::pty::{list_tmux_sessions, tmux_available, PtyTool, SessionId};

use super::AppState;

/// GET /api/tmux/sessions — list active tmux sessions and whether tmux is available.
pub async fn list_tmux_sessions_handler() -> Json<serde_json::Value> {
    let available = tmux_available();
    let sessions = if available { list_tmux_sessions() } else { vec![] };
    Json(serde_json::json!({
        "available": available,
        "sessions": sessions,
    }))
}

/// GET /api/agents — list enabled agents and default agent for frontend agent selector.
pub async fn list_agents_handler() -> Json<serde_json::Value> {
    let cfg = config::ensure_loaded();
    let agents: Vec<serde_json::Value> = cfg.enabled_agents.iter().map(|kind| {
        serde_json::json!({
            "id": kind.to_string(),
            "description": kind.description(),
        })
    }).collect();
    Json(serde_json::json!({
        "agents": agents,
        "default_agent": cfg.default_agent,
    }))
}

/// GET /api/services — list all services grouped by category.
pub async fn list_services_handler(State(state): State<AppState>) -> Json<common::service::StatusSnapshot> {
    Json(state.services.snapshot())
}

/// DELETE /api/services/:category/:id — kill a specific service.
pub async fn kill_service_handler(
    State(state): State<AppState>,
    Path((category, id)): Path<(String, String)>,
) -> impl IntoResponse {
    if state.services.kill_service(&category, &id) {
        (StatusCode::OK, format!("Killed {}/{}", category, id))
    } else {
        (StatusCode::NOT_FOUND, format!("Service {}/{} not found", category, id))
    }
}

/// Request body for POST /api/sessions.
#[derive(serde::Deserialize)]
pub(crate) struct CreateSessionBody {
    tool: PtyTool,
    project_path: Option<String>,
    tmux_session: Option<String>,
    theme: Option<String>,
    cols: Option<u16>,
    rows: Option<u16>,
}

/// GET /api/sessions — list all active sessions.
pub async fn list_sessions_handler(State(state): State<AppState>) -> Json<Vec<serde_json::Value>> {
    let items = state
        .pty_manager
        .list_sessions()
        .into_iter()
        .map(|item| serde_json::json!({
            "session_id": item.session_id,
            "tool": item.tool,
            "status": item.status,
            "created_at": item.created_at,
            "project_path": item.project_path,
            "tmux_session": item.tmux_session,
        }))
        .collect();
    Json(items)
}

/// POST /api/sessions — create a new PTY session.
pub async fn create_session_handler(
    State(state): State<AppState>,
    Json(body): Json<CreateSessionBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let initial_size = match (body.cols, body.rows) {
        (Some(c), Some(r)) => Some((c, r)),
        _ => None,
    };

    let created = state
        .pty_manager
        .create_session(
            body.tool,
            body.project_path.clone(),
            body.tmux_session.clone(),
            body.theme.clone(),
            initial_size,
        )
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok(Json(serde_json::json!({
        "session_id": created.session_id,
        "tool": created.tool,
        "created_at": created.created_at,
        "project_path": created.project_path,
    })))
}

/// DELETE /api/sessions/:session_id — kill and remove a session.
pub async fn delete_session_handler(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let uuid = match uuid::Uuid::parse_str(&session_id) {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid session_id".to_string()),
    };
    let sid = SessionId(uuid);
    if state.pty_manager.delete_session(sid) {
        (StatusCode::OK, format!("Session {} deleted", session_id))
    } else {
        (StatusCode::NOT_FOUND, format!("Session {} not found", session_id))
    }
}
