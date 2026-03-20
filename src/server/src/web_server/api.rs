//! REST API handlers for the web server.
//!
//! - GET /api/sessions
//! - POST /api/sessions
//! - DELETE /api/sessions/:session_id
//! - GET /api/tmux/sessions
//! - GET /api/agents
//! - GET /api/services
//! - DELETE /api/services/:category/:id

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};

use common::config;
use common::pty::{list_tmux_sessions, tmux_available, PtyTool};
use common::session::{self, SessionId};

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
    let mut items: Vec<serde_json::Value> = Vec::new();
    for entry in state.registry.iter() {
        let sid = entry.key();
        let ctx = entry.value();
        let status = ctx
            .state
            .read()
            .ok()
            .map(|g| serde_json::to_value(&*g).unwrap_or(serde_json::json!("unknown")))
            .unwrap_or(serde_json::json!("unknown"));
        items.push(serde_json::json!({
            "session_id": sid.0.to_string(),
            "tool": ctx.metadata.tool,
            "status": status,
            "created_at": ctx.metadata.created_at,
            "project_path": ctx.metadata.project_path,
            "tmux_session": ctx.metadata.tmux_session,
        }));
    }
    Json(items)
}

/// POST /api/sessions — create a new PTY session.
pub async fn create_session_handler(
    State(state): State<AppState>,
    Json(body): Json<CreateSessionBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let cwd = body.project_path.as_ref().map(std::path::PathBuf::from);
    let initial_size = match (body.cols, body.rows) {
        (Some(c), Some(r)) => Some((c, r)),
        _ => None,
    };

    let (bridge, mut pty_rx, resize_tx, mut state_rx) = common::pty::spawn_pty(
        body.tool,
        cwd,
        body.tmux_session.clone(),
        body.theme.clone(),
        initial_size,
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to spawn PTY: {}", e),
        )
    })?;

    let session_id = SessionId::new();
    let metadata = session::SessionMetadata {
        created_at: session::unix_now_secs(),
        project_path: body.project_path.clone(),
        tool: body.tool,
        tmux_session: body.tmux_session.clone(),
    };

    let buffer = Arc::new(session::CircularBuffer::new());
    let (live_tx, _) = tokio::sync::broadcast::channel(session::LIVE_BROADCAST_CAP);
    let run_state: Arc<std::sync::RwLock<common::pty::PtyRunState>> =
        Arc::new(std::sync::RwLock::new(common::pty::PtyRunState::Running {
            tool: body.tool,
        }));

    let ctx = session::SessionContext {
        bridge,
        resize_tx,
        state: Arc::clone(&run_state),
        metadata: metadata.clone(),
        buffer: Arc::clone(&buffer),
        live_tx: live_tx.clone(),
    };
    state.registry.insert(session_id, ctx);

    // Read PTY output and fan out to both ring buffer and WS subscribers.
    let buf_clone = Arc::clone(&buffer);
    let tx_clone = live_tx.clone();
    tokio::spawn(async move {
        while let Some(data) = pty_rx.recv().await {
            buf_clone.push(&data);
            let _ = tx_clone.send(bytes::Bytes::from(data));
        }
    });

    // Mirror PTY lifecycle state into session context.
    let rs = Arc::clone(&run_state);
    tokio::spawn(async move {
        while let Some(new_state) = state_rx.recv().await {
            if let Ok(mut g) = rs.write() {
                *g = new_state;
            }
        }
    });

    Ok(Json(serde_json::json!({
        "session_id": session_id.0.to_string(),
        "tool": metadata.tool,
        "created_at": metadata.created_at,
        "project_path": metadata.project_path,
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
    if let Some((_, ctx)) = state.registry.remove(&sid) {
        let _ = ctx.bridge.kill();
        (StatusCode::OK, format!("Session {} deleted", session_id))
    } else {
        (StatusCode::NOT_FOUND, format!("Session {} not found", session_id))
    }
}
