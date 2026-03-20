//! REST API handlers for the web server.
//!
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
use common::pty::{list_tmux_sessions, tmux_available};

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
