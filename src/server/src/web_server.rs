//! Axum HTTP + WebSocket server: serves Web SPA (from given dist path), WS at /ws for xterm ↔ PTY,
//! agent chat WS at /ws/chat, static preview (/preview/:project_id, /raw/:project_id/*),
//! and MCP endpoint at /mcp.

use axum::{
    extract::{Path, Query, State, ws::{Message, WebSocket, WebSocketUpgrade}},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{any, delete, get, post},
    Json, Router,
};
use axum::body::Body;
use bytes::Bytes;
use futures_util::stream::StreamExt;
use futures_util::SinkExt;
use std::io::Write;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::services::ServeDir;

use common::config;
use common::headless::wire;
use common::pty::{list_tmux_sessions, tmux_available, PtyTool};
use common::session::{self, Registry, SessionId};


/// Client sends this as JSON over Text frame to resize the PTY (e.g. after xterm-addon-fit).
#[derive(serde::Deserialize)]
struct ResizeMessage {
    #[serde(rename = "type")]
    ty: String,
    cols: u16,
    rows: u16,
}

/// Query params for /ws. session_id=uuid = attach to session; no session_id = legacy one-shot PTY (kill on disconnect).
#[derive(serde::Deserialize)]
struct WsQuery {
    session_id: Option<String>,
}

/// Shared app state: registry, SPA fallback path, working dir, optional Feishu webhook state, service manager.
#[derive(Clone)]
struct AppState {
    registry: Registry,
    dist_for_fallback: PathBuf,
    working_dir: PathBuf,
    feishu: Option<common::im::channels::feishu::FeishuWebhookState>,
    services: Arc<common::service::ServiceManager>,
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

/// Runs the Axum server (static files + WebSocket + session API). Binds to 127.0.0.1 (localhost only).
/// If feishu_state is Some, POST /api/im/feishu/event handles Feishu webhook (url_verification + events).
/// Call from desktop via tauri::async_runtime::spawn, or run standalone via the server binary.
pub async fn run_web_server(
    port: u16,
    dist_path: PathBuf,
    feishu_state: Option<common::im::channels::feishu::FeishuWebhookState>,
    services: Arc<common::service::ServiceManager>,
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
        feishu: feishu_state,
        services,
    };

    let app = Router::new()
        .route("/api/sessions", get(list_sessions_handler).post(create_session_handler))
        .route("/api/sessions/{session_id}", delete(delete_session_handler))
        .route("/api/tmux/sessions", get(list_tmux_sessions_handler))
        .route("/api/agents", get(list_agents_handler))
        .route("/api/im/feishu/event", post(feishu_webhook_handler))
        .route("/api/im/feishu/card", post(feishu_card_callback_handler))
        .route("/preview/{project_id}", get(preview_page_handler))
        .route("/raw/{project_id}", get(raw_root_handler))
        .route("/raw/{project_id}/{*path}", get(raw_path_handler))
        .route("/ws", get(ws_handler))
        .route("/ws/chat", get(ws_chat_handler))
        .route("/ws/services", get(ws_services_handler))
        .route("/api/services", get(list_services_handler))
        .route("/api/services/{category}/{id}", delete(kill_service_handler))
        .route("/mcp", post(mcp_handler))
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

async fn spa_fallback_handler(State(state): State<AppState>) -> Response {
    spa_fallback(state.dist_for_fallback.clone()).await
}

/// POST /api/im/feishu/event: Feishu sends url_verification (return {"challenge":"..."}) or event_callback.
async fn feishu_webhook_handler(
    State(state): State<AppState>,
    body: String,
) -> Response {
    let (status_code, body_str) = common::im::channels::feishu::handle_webhook_body(
        &body,
        state.feishu.as_ref(),
    )
    .await;
    let status = StatusCode::from_u16(status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        status,
        [("Content-Type", "application/json; charset=utf-8")],
        body_str,
    )
        .into_response()
}

/// POST /api/im/feishu/card: Feishu card button click callback.
async fn feishu_card_callback_handler(
    State(state): State<AppState>,
    body: String,
) -> Response {
    let (status_code, body_str) = common::im::channels::feishu::handle_card_callback(
        &body,
        state.feishu.as_ref(),
    )
    .await;
    let status = StatusCode::from_u16(status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        status,
        [("Content-Type", "application/json; charset=utf-8")],
        body_str,
    )
        .into_response()
}

async fn ws_handler(State(state): State<AppState>, Query(query): Query<WsQuery>, ws: WebSocketUpgrade) -> Response {
    if let Some(ref sid) = query.session_id {
        if let Ok(uuid) = uuid::Uuid::parse_str(sid) {
            let session_id = SessionId(uuid);
            let registry = state.registry.clone();
            return ws.on_upgrade(move |socket| handle_socket_attach(socket, session_id, registry));
        }
    }
    // session_id is required; reject bare /ws connections.
    ws.on_upgrade(|mut socket| async move {
        let _ = socket.send(Message::Text("Missing or invalid session_id".into())).await;
    })
}


/// GET /preview/:project_id — HTML page with iframe pointing to /raw/:project_id/
async fn preview_page_handler(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    let p = state.working_dir.join("workspaces").join(&project_id);
    if !p.exists() {
        return Err((StatusCode::NOT_FOUND, format!("Project not found: {}", project_id)));
    }
    let iframe_src = format!("/raw/{}", project_id);
    let html = format!(
        r#"<!DOCTYPE html><html><head><meta charset="utf-8"><title>Preview</title></head>
<body style="margin:0;overflow:hidden"><iframe src="{}" style="width:100%;height:100vh;border:0"></iframe></body></html>"#,
        iframe_src.replace('"', "&quot;")
    );
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/html; charset=utf-8")
        .body(Body::from(html))
        .unwrap())
}

async fn raw_impl(
    state: AppState,
    project_id: String,
    path: Option<String>,
) -> Result<Response, (StatusCode, String)> {
    let base = state.working_dir.join("workspaces").join(&project_id);
    if !base.exists() {
        return Err((StatusCode::NOT_FOUND, format!("Project not found: {}", project_id)));
    }
    let base = base
        .canonicalize()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let workspaces_root = state.working_dir.join("workspaces");
    let workspaces_root = workspaces_root
        .canonicalize()
        .unwrap_or_else(|_| state.working_dir.join("workspaces"));
    if !base.starts_with(&workspaces_root) {
        return Err((StatusCode::FORBIDDEN, "Invalid project path".into()));
    }
    let sub = path.as_deref().unwrap_or("").trim_start_matches('/');
    let requested = if sub.is_empty() {
        let index = base.join("index.html");
        if index.exists() {
            index
        } else {
            // No index.html: serve first .html file in directory (e.g. todo.html, todolist.html)
            let mut first_html: Option<std::path::PathBuf> = None;
            if let Ok(entries) = std::fs::read_dir(&base) {
                for e in entries.filter_map(|e| e.ok()) {
                    let p = e.path();
                    if p.is_file()
                        && p.file_name().and_then(|n| n.to_str()).map_or(false, |n| n.to_lowercase().ends_with(".html"))
                    {
                        first_html = Some(p);
                        break;
                    }
                }
            }
            first_html.unwrap_or_else(|| base.join("index.html"))
        }
    } else {
        let p = std::path::Path::new(sub);
        if p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Err((StatusCode::BAD_REQUEST, "Path traversal not allowed".into()));
        }
        base.join(p)
    };
    let requested = requested
        .canonicalize()
        .map_err(|_| (StatusCode::NOT_FOUND, "Not found".to_string()))?;
    if !requested.starts_with(&base) {
        return Err((StatusCode::FORBIDDEN, "Path outside workspace".into()));
    }
    if !requested.is_file() {
        return Err((StatusCode::NOT_FOUND, "Not found".to_string()));
    }
    let content = tokio::fs::read(&requested).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;
    let mime = mime_guess::from_path(&requested).first_raw().unwrap_or("application/octet-stream");
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", mime)
        .body(Body::from(content))
        .unwrap())
}

/// GET /raw/:project_id — serve index.html from project workspace.
async fn raw_root_handler(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    raw_impl(state, project_id, None).await
}

/// GET /raw/:project_id/*path — serve static file from project workspace (directory traversal safe).
async fn raw_path_handler(
    State(state): State<AppState>,
    Path((project_id, path)): Path<(String, String)>,
) -> Result<Response, (StatusCode, String)> {
    raw_impl(state, project_id, Some(path)).await
}

/// GET /api/tmux/sessions — list active tmux sessions and whether tmux is available.
async fn list_tmux_sessions_handler() -> Json<serde_json::Value> {
    let available = tmux_available();
    let sessions = if available { list_tmux_sessions() } else { vec![] };
    Json(serde_json::json!({
        "available": available,
        "sessions": sessions,
    }))
}

// ---------------------------------------------------------------------------
// Session CRUD API: GET /api/sessions, POST /api/sessions, DELETE /api/sessions/:id
// ---------------------------------------------------------------------------

/// Request body for POST /api/sessions.
#[derive(serde::Deserialize)]
struct CreateSessionBody {
    tool: PtyTool,
    project_path: Option<String>,
    tmux_session: Option<String>,
    theme: Option<String>,
    cols: Option<u16>,
    rows: Option<u16>,
}

/// GET /api/sessions — list all active sessions.
async fn list_sessions_handler(State(state): State<AppState>) -> Json<Vec<serde_json::Value>> {
    let mut items: Vec<serde_json::Value> = Vec::new();
    for entry in state.registry.iter() {
        let sid = entry.key();
        let ctx = entry.value();
        let status = ctx.state.read().ok().map(|g| {
            serde_json::to_value(&*g).unwrap_or(serde_json::json!("unknown"))
        }).unwrap_or(serde_json::json!("unknown"));
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
async fn create_session_handler(
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
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to spawn PTY: {}", e)))?;

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
        Arc::new(std::sync::RwLock::new(common::pty::PtyRunState::Running { tool: body.tool }));

    let ctx = session::SessionContext {
        bridge,
        resize_tx,
        state: Arc::clone(&run_state),
        metadata: metadata.clone(),
        buffer: Arc::clone(&buffer),
        live_tx: live_tx.clone(),
    };

    state.registry.insert(session_id, ctx);

    // Ghost reader: reads PTY output → buffer + broadcast.
    let buf_clone = Arc::clone(&buffer);
    let tx_clone = live_tx.clone();
    tokio::spawn(async move {
        while let Some(data) = pty_rx.recv().await {
            buf_clone.push(&data);
            let _ = tx_clone.send(bytes::Bytes::from(data));
        }
    });

    // State watcher: updates run_state from PTY child poll.
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
async fn delete_session_handler(
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

/// GET /api/agents — list enabled agents and default agent for frontend agent selector.
async fn list_agents_handler() -> Json<serde_json::Value> {
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



// ---------------------------------------------------------------------------
// WebSocket: /ws/services — real-time service status push
// ---------------------------------------------------------------------------

async fn ws_services_handler(
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| ws_services_session(socket, state.services))
}

async fn ws_services_session(
    mut socket: axum::extract::ws::WebSocket,
    services: Arc<common::service::ServiceManager>,
) {
    use axum::extract::ws::Message;

    // 1. Send initial snapshot immediately
    let snapshot = services.list_all();
    if let Ok(json) = serde_json::to_string(&snapshot) {
        if socket.send(Message::Text(json.into())).await.is_err() {
            return;
        }
    }

    // 2. Subscribe to changes and forward
    let mut rx = services.change_tx.subscribe();
    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(snapshot) => {
                        if let Ok(json) = serde_json::to_string(&snapshot) {
                            if socket.send(Message::Text(json.into())).await.is_err() {
                                break; // client disconnected
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        eprintln!("[VibeAround][ws/services] lagged by {}, sending fresh snapshot", n);
                        let snapshot = services.list_all();
                        if let Ok(json) = serde_json::to_string(&snapshot) {
                            if socket.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            // Also listen for client messages (ping/pong/close)
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(data))) => {
                        let _ = socket.send(Message::Pong(data)).await;
                    }
                    _ => {} // ignore text/binary from client
                }
            }
        }
    }
}

async fn ws_chat_handler(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    let working_dir = state.working_dir.clone();
    ws.on_upgrade(move |socket| handle_chat_socket(socket, working_dir))
}

async fn handle_socket_attach(mut socket: WebSocket, session_id: SessionId, registry: Registry) {
    let (buffer, state, live_tx, writer, resize_tx) = {
        let ctx = match registry.get(&session_id) {
            Some(c) => c,
            None => {
                let _ = socket.send(Message::Text("Session not found".into())).await;
                return;
            }
        };
        (
            ctx.buffer.clone(),
            ctx.state.clone(),
            ctx.live_tx.clone(),
            ctx.bridge.writer.clone(),
            ctx.resize_tx.clone(),
        )
    };
    let (mut ws_tx, mut ws_rx) = socket.split();
    let dump = buffer.dump();
    if !dump.is_empty() {
        let _ = ws_tx.send(Message::Binary(Bytes::from(dump))).await;
    }
    let state_json = state.read().ok().and_then(|g| serde_json::to_string(&*g).ok());
    if let Some(json) = state_json {
        let _ = ws_tx.send(Message::Text(json.into())).await;
    }
    let mut live_rx = live_tx.subscribe();

    let live_to_ws = async {
        while let Ok(bytes) = live_rx.recv().await {
            if ws_tx.send(Message::Binary(bytes)).await.is_err() {
                break;
            }
        }
    };
    let ws_to_pty = async move {
        while let Some(Ok(msg)) = ws_rx.next().await {
            match &msg {
                Message::Text(t) => {
                    if let Ok(resize) = serde_json::from_str::<ResizeMessage>(t) {
                        if resize.ty == "resize" {
                            let _ = resize_tx.send((resize.cols, resize.rows));
                            continue;
                        }
                    }
                    let to_write = t.as_bytes().to_vec();
                    let w = writer.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        if let Ok(mut guard) = w.lock() {
                            let _ = guard.write_all(&to_write);
                            let _ = guard.flush();
                        }
                    })
                    .await;
                }
                Message::Binary(b) => {
                    let to_write = b.to_vec();
                    let w = writer.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        if let Ok(mut guard) = w.lock() {
                            let _ = guard.write_all(&to_write);
                            let _ = guard.flush();
                        }
                    })
                    .await;
                }
                _ => {}
            }
        }
    };
    tokio::select! {
        _ = live_to_ws => {}
        _ = ws_to_pty => {}
    }
}


async fn handle_chat_socket(socket: WebSocket, working_dir: PathBuf) {
    use common::agent::{self, AgentBackend, AgentEvent, AgentKind};

    let (mut ws_tx, mut ws_rx) = socket.split();

    // Push enabled agents config to client on connect
    let default_agent_str = {
        let cfg = config::ensure_loaded();
        let agents: Vec<serde_json::Value> = cfg.enabled_agents.iter().map(|kind| {
            serde_json::json!({
                "id": kind.to_string(),
                "description": kind.description(),
            })
        }).collect();
        let config_msg = serde_json::json!({
            "type": "config",
            "agents": agents,
            "default_agent": cfg.default_agent,
        });
        let _ = ws_tx.send(Message::Text(config_msg.to_string().into())).await;
        cfg.default_agent.clone()
    };

    let mut active_agent: Option<Box<dyn AgentBackend>> = None;

    /// Start or switch agent backend. Returns true on success.
    async fn start_agent(
        active_agent: &mut Option<Box<dyn AgentBackend>>,
        kind: AgentKind,
        cwd: &std::path::Path,
    ) -> Result<(), String> {
        // Shut down existing
        if let Some(mut old) = active_agent.take() {
            old.shutdown().await;
        }
        let mut backend = agent::create_backend(kind);
        backend.start(cwd, None).await?;
        *active_agent = Some(backend);
        Ok(())
    }

    while let Some(Ok(msg)) = ws_rx.next().await {
        let Message::Text(user_msg) = msg else { continue };
        let prompt = user_msg.trim().to_string();
        if prompt.is_empty() {
            continue;
        }

        // Handle /cli_<agent> command — switch agent
        if let Some(rest) = prompt.strip_prefix("/cli_") {
            if let Some(kind) = AgentKind::from_str_loose(rest.trim()) {
                if kind.is_enabled() {
                    let _ = ws_tx.send(Message::Text(
                        wire::text_json(&format!("Switching to {} agent...\n", kind)).into()
                    )).await;
                    match start_agent(&mut active_agent, kind, &working_dir).await {
                        Ok(()) => {
                            let _ = ws_tx.send(Message::Text(
                                wire::text_json(&format!("{} agent started ✅\n", kind)).into()
                            )).await;
                            // Notify frontend of the switch
                            let switch_msg = serde_json::json!({
                                "type": "agent_switched",
                                "agent": kind.to_string(),
                            });
                            let _ = ws_tx.send(Message::Text(switch_msg.to_string().into())).await;
                        }
                        Err(e) => {
                            let _ = ws_tx.send(Message::Text(
                                wire::error_json(&format!("Failed to start {}: {}", kind, e)).into()
                            )).await;
                        }
                    }
                    let _ = ws_tx.send(Message::Text(wire::done_json().into())).await;
                    continue;
                }
            }
            // Unknown or disabled agent
            let _ = ws_tx.send(Message::Text(
                wire::error_json("Unknown or disabled agent").into()
            )).await;
            let _ = ws_tx.send(Message::Text(wire::done_json().into())).await;
            continue;
        }

        // Lazy-start default agent on first real message
        if active_agent.is_none() {
            let default_kind = AgentKind::from_str_loose(&default_agent_str)
                .unwrap_or(AgentKind::Claude);
            if let Err(e) = start_agent(&mut active_agent, default_kind, &working_dir).await {
                let _ = ws_tx.send(Message::Text(
                    wire::error_json(&format!("Failed to start default agent: {}", e)).into()
                )).await;
                let _ = ws_tx.send(Message::Text(wire::done_json().into())).await;
                continue;
            }
        }

        // Send message to active agent and stream events back
        let agent = active_agent.as_ref().unwrap();
        let mut rx = agent.subscribe();

        if let Err(e) = agent.send_message_fire(&prompt).await {
            // Check if agent died — try to restart
            let is_dead = e.contains("shut down") || e.contains("gone") || e.contains("ACP thread");
            let _ = ws_tx.send(Message::Text(
                wire::error_json(&e).into()
            )).await;
            if is_dead {
                let kind = agent.kind();
                let _ = ws_tx.send(Message::Text(
                    wire::text_json(&format!("⚠️ {} agent crashed, restarting...\n", kind)).into()
                )).await;
                if let Ok(()) = start_agent(&mut active_agent, kind, &working_dir).await {
                    let _ = ws_tx.send(Message::Text(
                        wire::text_json(&format!("{} agent restarted ✅\n", kind)).into()
                    )).await;
                    // Retry the message
                    let agent = active_agent.as_ref().unwrap();
                    rx = agent.subscribe();
                    if let Err(e2) = agent.send_message_fire(&prompt).await {
                        let _ = ws_tx.send(Message::Text(wire::error_json(&e2).into())).await;
                        let _ = ws_tx.send(Message::Text(wire::done_json().into())).await;
                        continue;
                    }
                } else {
                    let _ = ws_tx.send(Message::Text(wire::done_json().into())).await;
                    continue;
                }
            } else {
                let _ = ws_tx.send(Message::Text(wire::done_json().into())).await;
                continue;
            }
        }

        // Stream agent events to WebSocket
        loop {
            match rx.recv().await {
                Ok(event) => match event {
                    AgentEvent::Text(text) => {
                        let _ = ws_tx.send(Message::Text(wire::text_json(&text).into())).await;
                    }
                    AgentEvent::Thinking(_) => {
                        // Web chat doesn't show thinking blocks for now
                    }
                    AgentEvent::Progress(status) => {
                        let json = serde_json::json!({ "progress": status }).to_string();
                        let _ = ws_tx.send(Message::Text(json.into())).await;
                    }
                    AgentEvent::ToolUse { name, .. } => {
                        let json = serde_json::json!({ "progress": format!("Using tool: {}...", name) }).to_string();
                        let _ = ws_tx.send(Message::Text(json.into())).await;
                    }
                    AgentEvent::ToolResult { .. } => {}
                    AgentEvent::TurnComplete { .. } => {
                        break;
                    }
                    AgentEvent::Error(err) => {
                        let _ = ws_tx.send(Message::Text(wire::error_json(&err).into())).await;
                    }
                },
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("[VibeAround][ws/chat] event stream lagged by {}", n);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    let _ = ws_tx.send(Message::Text(
                        wire::error_json("Agent process ended unexpectedly").into()
                    )).await;
                    break;
                }
            }
        }

        let _ = ws_tx.send(Message::Text(wire::done_json().into())).await;
    }

    // Clean up agent on disconnect
    if let Some(mut agent) = active_agent.take() {
        agent.shutdown().await;
    }
}

// ---------------------------------------------------------------------------
// Service management API
// ---------------------------------------------------------------------------

/// GET /api/services — list all services grouped by category.
async fn list_services_handler(State(state): State<AppState>) -> Json<common::service::ServicesSnapshot> {
    Json(state.services.list_all())
}

/// DELETE /api/services/:category/:id — kill a specific service.
async fn kill_service_handler(
    State(state): State<AppState>,
    Path((category, id)): Path<(String, String)>,
) -> impl IntoResponse {
    if state.services.kill(&category, &id) {
        (StatusCode::OK, format!("Killed {}/{}", category, id))
    } else {
        (StatusCode::NOT_FOUND, format!("Service {}/{} not found", category, id))
    }
}

// ---------------------------------------------------------------------------
// MCP Streamable HTTP endpoint — POST /mcp
// ---------------------------------------------------------------------------

/// JSON-RPC 2.0 request envelope.
#[derive(serde::Deserialize)]
struct JsonRpcRequest {
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
async fn mcp_handler(
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
    state: &AppState,
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
    let message_with_date = format!("[Current date: {}]\n\n{}", date_str, message);
    let kind = arguments
        .get("kind")
        .and_then(|v| v.as_str())
        .and_then(common::agent::AgentKind::from_str_loose);

    // Dispatch to registry
    match common::agent::registry::dispatch_task(&state.services, workspace, &message_with_date, kind).await {
        Ok(result) => {
            let summary = format!(
                "Task completed by worker {}. The user already saw the worker's output in real-time — do NOT repeat it.",
                result.agent_id
            );
            jsonrpc_ok(id, serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": summary
                }],
                "isError": false,
                "_meta": {
                    "agent_id": result.agent_id,
                    "spawned": result.spawned
                }
            }))
        }
        Err(e) => {
            jsonrpc_ok(id, serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": format!("Error: {}", e)
                }],
                "isError": true
            }))
        }
    }
}

