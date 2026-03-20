//! Axum HTTP + WebSocket server: serves Web SPA (from given dist path), WS at /ws for xterm ↔ PTY,
//! agent chat WS at /ws/chat, static preview (/preview/:project_id, /raw/:project_id/*),
//! and MCP endpoint at /mcp.

mod api;
mod mcp;
mod preview;
mod ws_chat;
mod ws_pty;
mod ws_services;

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{any, delete, get, post},
    Router,
};
use axum::body::Body;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::services::ServeDir;

use common::config;
use common::session::Registry;

/// Client sends this as JSON over Text frame to resize the PTY (e.g. after xterm-addon-fit).
#[derive(serde::Deserialize)]
struct ResizeMessage {
    #[serde(rename = "type")]
    ty: String,
    cols: u16,
    rows: u16,
}

/// Query params for /ws. session_id=uuid = attach to session.
#[derive(serde::Deserialize)]
struct WsQuery {
    session_id: Option<String>,
}

/// Shared app state: registry, SPA fallback path, working dir, service manager.
#[derive(Clone)]
pub(crate) struct AppState {
    registry: Registry,
    dist_for_fallback: PathBuf,
    working_dir: PathBuf,
    services: Arc<common::service::ServiceStatusManager>,
}

/// Ensure web dist exists (build web first).
fn verify_web_dist(web_dist: &std::path::Path) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if !web_dist.exists() {
        eprintln!("[VibeAround] Web dist not found: {:?}", web_dist);
        return Err(format!("Web dist not found: {:?}", web_dist).into());
    }
    if !web_dist.join("index.html").exists() {
        eprintln!("[VibeAround] index.html not found in {:?}", web_dist);
        return Err(format!("index.html not found in {:?}", web_dist).into());
    }
    Ok(())
}

async fn spa_fallback(dist_path: PathBuf) -> Response {
    let index_path = dist_path.join("index.html");
    match tokio::fs::read_to_string(&index_path).await {
        Ok(content) => Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "text/html; charset=utf-8")
            .body(Body::from(content))
            .unwrap(),
        Err(e) => {
            eprintln!("[VibeAround] Failed to read index.html: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to load index.html: {}", e)).into_response()
        }
    }
}

async fn spa_fallback_handler(axum::extract::State(state): axum::extract::State<AppState>) -> Response {
    spa_fallback(state.dist_for_fallback.clone()).await
}

/// Runs the Axum server (static files + WebSocket + session API). Binds to 127.0.0.1 (localhost only).
/// Call from desktop via tauri::async_runtime::spawn, or run standalone via the server binary.
pub async fn run_web_server(
    port: u16,
    dist_path: PathBuf,
    services: Arc<common::service::ServiceStatusManager>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    verify_web_dist(&dist_path)?;
    let web_dist = dist_path
        .canonicalize()
        .map_err(|e| format!("Failed to resolve web dist path: {}", e))?;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    println!(
        "[VibeAround] Web dashboard: http://127.0.0.1:{}, serving from {:?}",
        port, web_dist
    );

    let assets_dir = web_dist.join("assets");
    let working_dir = config::ensure_loaded().working_dir.clone();
    let state = AppState {
        registry: Arc::clone(&services.pty),
        dist_for_fallback: web_dist.clone(),
        working_dir,
        services,
    };

    let app = Router::new()
        .route("/api/sessions", get(api::list_sessions_handler).post(api::create_session_handler))
        .route("/api/sessions/{session_id}", delete(api::delete_session_handler))
        .route("/api/tmux/sessions", get(api::list_tmux_sessions_handler))
        .route("/api/agents", get(api::list_agents_handler))
        .route("/preview/{project_id}", get(preview::preview_page_handler))
        .route("/raw/{project_id}", get(preview::raw_root_handler))
        .route("/raw/{project_id}/{*path}", get(preview::raw_path_handler))
        .route("/ws", get(ws_pty::ws_handler))
        .route("/ws/chat", get(ws_chat::ws_chat_handler))
        .route("/ws/services", get(ws_services::ws_services_handler))
        .route("/api/services", get(api::list_services_handler))
        .route("/api/services/{category}/{id}", delete(api::kill_service_handler))
        .route("/mcp", post(mcp::mcp_handler))
        .nest_service("/assets", ServeDir::new(assets_dir))
        .fallback(any(spa_fallback_handler))
        .with_state(state)
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any),
        );

    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::AddrInUse {
            eprintln!(
                "[VibeAround] ⚠️  Port {} is already in use — is another VibeAround instance running?",
                port
            );
        }
        e
    })?;
    println!("[VibeAround] Web server listening on http://127.0.0.1:{}", port);
    axum::serve(listener, app).await?;
    Ok(())
}
