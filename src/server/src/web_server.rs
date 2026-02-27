//! Axum HTTP + WebSocket server: serves Web SPA (from given dist path), WS at /ws for xterm ↔ PTY,
//! session registry API (POST/GET/DELETE /api/sessions), job workspace API (/api/jobs),
//! and static preview (/preview/:job_id, /raw/:job_id/*). /ws?session_id=xxx attaches to a session.

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
use std::sync::{Arc, RwLock};
use tower_http::services::ServeDir;
use tokio::sync::broadcast;

use common::config;
use common::headless::{run_claude_prompt_to_stream_parts, wire, ClaudeSegment};
use common::pty::{PtyRunState, PtyTool, list_tmux_sessions, tmux_available};
use common::session::{
    CircularBuffer, Registry, SessionContext, SessionId, SessionMetadata, LIVE_BROADCAST_CAP,
    unix_now_secs,
};
use common::workspace::{self, JobRecord};

const DELAYED_RELEASE_SECS: u64 = 600; // 10 minutes after Exited before removing from registry

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

/// Shared app state: registry, SPA fallback path, working dir for jobs, optional Feishu webhook state.
#[derive(Clone)]
struct AppState {
    registry: Registry,
    dist_for_fallback: PathBuf,
    working_dir: PathBuf,
    feishu: Option<common::im::channels::feishu::FeishuWebhookState>,
}

/// POST /api/sessions body.
#[derive(serde::Deserialize)]
struct CreateSessionBody {
    tool: String,
    #[serde(default)]
    project_path: Option<String>,
    /// If set, session PTY runs in this job's workspace directory.
    #[serde(default)]
    job_id: Option<String>,
    /// If set, spawn inside a tmux session with this name (attach-or-create).
    #[serde(default)]
    tmux_session: Option<String>,
}

/// POST /api/jobs body.
#[derive(serde::Deserialize)]
struct CreateJobBody {
    name: String,
    #[serde(default)]
    description: String,
}

/// Session list item (GET /api/sessions).
#[derive(serde::Serialize)]
struct SessionListItem {
    session_id: String,
    tool: String,
    status: String, // "running" | "exited"
    created_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    project_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tmux_session: Option<String>,
}

fn parse_tool(s: Option<&String>) -> PtyTool {
    let t = s.as_deref().map(|x| x.to_lowercase());
    match t.as_deref() {
        Some("claude") => PtyTool::Claude,
        Some("gemini") => PtyTool::Gemini,
        Some("codex") => PtyTool::Codex,
        _ => PtyTool::Generic,
    }
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
    if let Err(e) = workspace::ensure_workspace_dirs(&working_dir) {
        eprintln!("[VibeAround] Failed to create workspaces dir: {}", e);
    }
    let state = AppState {
        registry: Arc::new(dashmap::DashMap::new()),
        dist_for_fallback: web_dist.clone(),
        working_dir,
        feishu: feishu_state,
    };

    let app = Router::new()
        .route("/api/sessions", get(list_sessions_handler).post(create_session_handler))
        .route("/api/sessions/{id}", delete(delete_session_handler))
        .route("/api/tmux/sessions", get(list_tmux_sessions_handler))
        .route("/api/jobs", get(list_jobs_handler).post(create_job_handler))
        .route("/api/im/feishu/event", post(feishu_webhook_handler))
        .route("/preview/{job_id}", get(preview_page_handler))
        .route("/raw/{job_id}", get(raw_job_root_handler))
        .route("/raw/{job_id}/{*path}", get(raw_job_path_handler))
        .route("/ws", get(ws_handler))
        .route("/ws/chat", get(ws_chat_handler))
        .nest_service("/assets", ServeDir::new(assets_dir))
        .fallback(any(spa_fallback_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
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

async fn create_session_handler(
    State(state): State<AppState>,
    Json(body): Json<CreateSessionBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let tool = parse_tool(Some(&body.tool));

    // Note: no server-side dedup for tmux sessions. The frontend checks if a tab
    // already exists and switches to it. When the user explicitly requests a new
    // attach (e.g. after rebuild or to re-detach others), we always spawn a fresh PTY.

    let (cwd, project_path) = if let Some(ref job_id) = body.job_id {
        match workspace::job_workspace_path(&state.working_dir, job_id) {
            Some(p) => (Some(p.clone()), Some(p.to_string_lossy().into_owned())),
            None => {
                return Err((
                    StatusCode::NOT_FOUND,
                    format!("Job not found: {}", job_id),
                ));
            }
        }
    } else {
        (None, body.project_path.clone())
    };
    let (bridge, mut pty_rx, resize_tx, mut state_rx) =
        common::pty::spawn_pty(tool, cwd, body.tmux_session.clone()).map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to start PTY: {}", e))
        })?;
    let created_at = unix_now_secs();
    let metadata = SessionMetadata {
        created_at,
        project_path,
        tool,
        tmux_session: body.tmux_session.clone(),
    };
    let buffer = Arc::new(CircularBuffer::new());
    let (live_tx, _) = broadcast::channel::<Bytes>(LIVE_BROADCAST_CAP);
    let run_state = Arc::new(RwLock::new(PtyRunState::Running { tool }));
    let session_id = SessionId::new();
    let ctx = SessionContext {
        bridge,
        resize_tx: resize_tx.clone(),
        state: run_state.clone(),
        metadata: metadata.clone(),
        buffer: buffer.clone(),
        live_tx: live_tx.clone(),
    };
    state.registry.insert(session_id, ctx);

    tokio::spawn({
        let buffer = Arc::clone(&buffer);
        let live_tx = live_tx.clone();
        async move {
            while let Some(d) = pty_rx.recv().await {
                buffer.push(&d);
                let _ = live_tx.send(Bytes::from(d));
            }
        }
    });
    tokio::spawn({
        let registry = state.registry.clone();
        let run_state = run_state.clone();
        async move {
            while let Some(s) = state_rx.recv().await {
                if let Ok(mut g) = run_state.write() {
                    *g = s.clone();
                }
                if let PtyRunState::Exited { .. } = s {
                    let sid = session_id;
                    let reg = registry.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(DELAYED_RELEASE_SECS)).await;
                        if let Some((_, ctx)) = reg.remove(&sid) {
                            let _ = ctx.bridge.kill();
                        }
                    });
                    break;
                }
            }
        }
    });

    Ok(Json(serde_json::json!({
        "session_id": session_id.to_string(),
        "tool": format!("{:?}", tool).to_lowercase(),
        "created_at": created_at,
        "project_path": metadata.project_path,
    })))
}

async fn list_jobs_handler(State(state): State<AppState>) -> Json<Vec<JobRecord>> {
    let list = workspace::list_jobs(&state.working_dir);
    Json(list)
}

async fn create_job_handler(
    State(state): State<AppState>,
    Json(body): Json<CreateJobBody>,
) -> Result<Json<JobRecord>, (StatusCode, String)> {
    let record = workspace::create_job(
        &state.working_dir,
        body.name,
        body.description,
    )
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(record))
}

/// GET /preview/:job_id — HTML page with iframe pointing to /raw/:job_id/
async fn preview_page_handler(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    let _job = workspace::get_job(&state.working_dir, &job_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("Job not found: {}", job_id)))?;
    let iframe_src = format!("/raw/{}", job_id);
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

async fn raw_job_impl(
    state: AppState,
    job_id: String,
    path: Option<String>,
) -> Result<Response, (StatusCode, String)> {
    let base = workspace::job_workspace_path(&state.working_dir, &job_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("Job not found: {}", job_id)))?;
    let base = base
        .canonicalize()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let workspaces_root = state.working_dir.join("workspaces");
    let workspaces_root = workspaces_root
        .canonicalize()
        .unwrap_or_else(|_| state.working_dir.join("workspaces"));
    if !base.starts_with(&workspaces_root) {
        return Err((StatusCode::FORBIDDEN, "Invalid job path".into()));
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

/// GET /raw/:job_id — serve index.html from job workspace.
async fn raw_job_root_handler(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    raw_job_impl(state, job_id, None).await
}

/// GET /raw/:job_id/*path — serve static file from job workspace (directory traversal safe).
async fn raw_job_path_handler(
    State(state): State<AppState>,
    Path((job_id, path)): Path<(String, String)>,
) -> Result<Response, (StatusCode, String)> {
    raw_job_impl(state, job_id, Some(path)).await
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

async fn list_sessions_handler(State(state): State<AppState>) -> Json<Vec<SessionListItem>> {
    let list: Vec<_> = state
        .registry
        .iter()
        .map(|r| {
            let id = r.key();
            let ctx = r.value();
            let status = match ctx.state.read() {
                Ok(g) => match &*g {
                    PtyRunState::Running { .. } => "running",
                    PtyRunState::Exited { .. } => "exited",
                },
                _ => "unknown",
            };
            SessionListItem {
                session_id: id.to_string(),
                tool: format!("{:?}", ctx.metadata.tool).to_lowercase(),
                status: status.to_string(),
                created_at: ctx.metadata.created_at,
                project_path: ctx.metadata.project_path.clone(),
                tmux_session: ctx.metadata.tmux_session.clone(),
            }
        })
        .collect();
    Json(list)
}

async fn delete_session_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let uuid = uuid::Uuid::parse_str(&id).map_err(|_| (StatusCode::BAD_REQUEST, "Invalid session_id".into()))?;
    let sid = SessionId(uuid);
    if let Some((_, ctx)) = state.registry.remove(&sid) {
        let _ = ctx.bridge.kill();
    }
    Ok(StatusCode::NO_CONTENT)
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
    let (mut ws_tx, mut ws_rx) = socket.split();
    let mut job_id: Option<String> = None;

    while let Some(Ok(msg)) = ws_rx.next().await {
        let Message::Text(user_msg) = msg else { continue };
        let prompt = user_msg.trim().to_string();
        if prompt.is_empty() {
            continue;
        }

        if job_id.is_none() {
            let name = prompt.chars().take(50).collect::<String>();
            let name = if name.is_empty() { "Chat".into() } else { name };
            match workspace::create_job(&working_dir, name, String::new()) {
                Ok(record) => {
                    job_id = Some(record.job_id.clone());
                    let preview = format!("/preview/{}", record.job_id);
                    let _ = ws_tx
                        .send(Message::Text(wire::job_json(&record.job_id, &preview).into()))
                        .await;
                }
                Err(e) => {
                    let _ = ws_tx
                        .send(Message::Text(wire::error_json(&format!("Failed to create job: {}", e)).into()))
                        .await;
                    let _ = ws_tx
                        .send(Message::Text(wire::done_json().into()))
                        .await;
                    continue;
                }
            }
        }

        let cwd = match job_id.as_ref().and_then(|id| workspace::job_workspace_path(&working_dir, id)) {
            Some(p) => p,
            None => {
                let _ = ws_tx
                    .send(Message::Text(wire::error_json("Job workspace not found").into()))
                    .await;
                continue;
            }
        };

        let (seg_tx, mut seg_rx) = tokio::sync::mpsc::channel::<ClaudeSegment>(64);
        let prompt = prompt.clone();
        let cwd = Some(cwd);
        let run_result = tokio::spawn(async move {
            run_claude_prompt_to_stream_parts(&prompt, move |seg| {
                let _ = seg_tx.try_send(seg);
            }, cwd, None).await
        });

        while let Some(seg) = seg_rx.recv().await {
            let json = wire::segment_to_json(&seg);
            let _ = ws_tx.send(Message::Text(json.into())).await;
        }

        match run_result.await {
            Ok(Ok(_runner_result)) => {}
            Ok(Err(e)) => {
                let _ = ws_tx
                    .send(Message::Text(wire::error_json(&format!("Failed to run claude: {}", e)).into()))
                    .await;
            }
            Err(e) => {
                let _ = ws_tx
                    .send(Message::Text(wire::error_json(&format!("Task join error: {}", e)).into()))
                    .await;
            }
        }

        let _ = ws_tx
            .send(Message::Text(wire::done_json().into()))
            .await;
    }
}
