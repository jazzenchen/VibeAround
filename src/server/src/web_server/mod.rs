//! Axum HTTP + WebSocket server: serves Web SPA (from given dist path), WS at /ws for xterm ↔ PTY,
//! agent chat WS at /ws/chat, live preview (/preview/:slug with iframe wrapper + reverse proxy),
//! and MCP endpoint at /mcp.

mod agent_hooks;
mod api;
mod api_bridge;
mod auth;
mod mcp;
mod pair;
mod preview;
mod ws_chat;
mod ws_domains;
mod ws_pty;

use axum::body::Body;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, delete, get, post};
use axum::Router;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::services::ServeDir;

use crate::agent_hooks::AgentHookRegistry;
use common::auth::AuthToken;
use common::channels::{ChannelManager, WebChannelManager};
use common::pty::{PtySessionManager, Registry};
use common::tunnels::TunnelManager;

use self::auth::{require_auth, AuthState};

const LOCAL_BRIDGE_BODY_LIMIT_BYTES: usize = 64 * 1024 * 1024;

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
    #[serde(default)]
    #[allow(dead_code)] // consumed by the auth middleware, not the handler
    token: Option<String>,
}

/// Shared app state: per-domain manager handles + server metadata.
#[derive(Clone)]
pub(crate) struct AppState {
    pty_manager: Arc<PtySessionManager>,
    dist_for_fallback: PathBuf,
    tunnels: Arc<TunnelManager>,
    channel_hub: Arc<ChannelManager>,
    web_channel: Arc<WebChannelManager>,
    /// Port the daemon is bound to. Handlers that need to build
    /// loopback URLs use this instead of reaching into a services
    /// facade.
    port: u16,
    /// Shared HTTP client for preview proxy and API bridge forwarding.
    preview_client: reqwest::Client,
    /// Codex/agent lifecycle events received from bundled hook helpers.
    hook_registry: Arc<AgentHookRegistry>,
}

/// Ensure web dist exists (build web first).
fn verify_web_dist(
    web_dist: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if !web_dist.exists() {
        tracing::info!("[VibeAround] Web dist not found: {:?}", web_dist);
        return Err(format!("Web dist not found: {:?}", web_dist).into());
    }
    if !web_dist.join("index.html").exists() {
        tracing::info!("[VibeAround] index.html not found in {:?}", web_dist);
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
            .unwrap_or_else(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to build response",
                )
                    .into_response()
            }),
        Err(e) => {
            tracing::info!("[VibeAround] Failed to read index.html: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load index.html: {}", e),
            )
                .into_response()
        }
    }
}

async fn spa_fallback_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
) -> Response {
    if is_dashboard_api_path(uri.path()) {
        return (
            StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({
                "error": {
                    "message": "API route not found",
                    "type": "vibearound_bridge_error",
                }
            })),
        )
            .into_response();
    }

    spa_fallback(state.dist_for_fallback.clone()).await
}

fn is_dashboard_api_path(path: &str) -> bool {
    ["/va/local-api/", "/local-api/", "/va/bridge/", "/bridge/"]
        .into_iter()
        .any(|prefix| path.starts_with(prefix))
}

/// Runs the Axum server (static files + WebSocket + session API). Binds to 127.0.0.1 (localhost only).
/// Call from desktop via tauri::async_runtime::spawn, or run standalone via the server binary.
pub async fn run_web_server(
    port: u16,
    dist_path: PathBuf,
    tunnels: Arc<TunnelManager>,
    pty_registry: Registry,
    channel_hub: Arc<ChannelManager>,
    web_channel: Arc<WebChannelManager>,
    auth_token: Arc<AuthToken>,
    hook_registry: Arc<AgentHookRegistry>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    verify_web_dist(&dist_path)?;
    let web_dist = dist_path
        .canonicalize()
        .map_err(|e| format!("Failed to resolve web dist path: {}", e))?;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    println!(
        "[VibeAround] Web dashboard: http://127.0.0.1:{}/va/, serving from {:?}",
        port, web_dist
    );

    let assets_dir = web_dist.join("assets");
    let brand_dir = web_dist.join("brand");
    let preview_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("reqwest client");
    let state = AppState {
        pty_manager: Arc::new(PtySessionManager::from_registry(pty_registry)),
        dist_for_fallback: web_dist.clone(),
        tunnels,
        channel_hub,
        web_channel,
        port,
        preview_client,
        hook_registry,
    };

    let auth_state = AuthState(Arc::clone(&auth_token));

    // --- Protected routes: require a valid token on every request. ----------
    //
    // Everything that can mutate state, execute code, attach to a PTY, or
    // surface sensitive workspace data goes behind the auth middleware.
    // The SPA shell + static assets are left open so the browser can fetch
    // the initial HTML/JS with the token supplied as `?token=...` by the
    // opener (Tauri tray). The SPA then attaches `Authorization: Bearer` on
    // every subsequent API/WS call.
    let protected = Router::new()
        .route(
            "/api/sessions",
            get(api::list_sessions_handler).post(api::create_session_handler),
        )
        .route(
            "/api/sessions/{session_id}",
            delete(api::delete_session_handler),
        )
        .route(
            "/api/launch-sessions",
            post(api::list_launch_sessions_batch_handler),
        )
        .route(
            "/api/agents/{agent_id}/launch-sessions",
            get(api::list_launch_sessions_handler),
        )
        .route(
            "/api/agents/{agent_id}/launch-sessions/{session_id}/archive",
            post(api::archive_launch_session_handler)
                .delete(api::unarchive_launch_session_delete_handler),
        )
        .route(
            "/api/agents/{agent_id}/launch-sessions/{session_id}/unarchive",
            post(api::unarchive_launch_session_handler),
        )
        .route(
            "/api/chat/uploads",
            post(api::upload_chat_file_handler)
                .layer(DefaultBodyLimit::max(LOCAL_BRIDGE_BODY_LIMIT_BYTES)),
        )
        .route(
            "/api/chat/files/download",
            get(api::download_chat_file_handler),
        )
        .route("/api/tmux/sessions", get(api::list_tmux_sessions_handler))
        .route("/api/agents", get(api::list_agents_handler))
        .route("/api/profiles", get(api::list_profiles_handler))
        .route("/ws", get(ws_pty::ws_handler))
        .route("/ws/chat", get(ws_chat::ws_chat_handler))
        .route("/ws/channels", get(ws_domains::ws_channels_handler))
        .route("/ws/tunnels", get(ws_domains::ws_tunnels_handler))
        .route(
            "/ws/agents/runtime",
            get(ws_domains::ws_agents_runtime_handler),
        )
        .route("/api/channels", get(api::list_channels_handler))
        .route("/api/channels/sync", post(api::sync_channels_handler))
        .route("/api/settings/reload", post(api::reload_settings_handler))
        .route("/api/tunnels", get(api::list_tunnels_handler))
        .route("/api/agents/runtime", get(api::list_agents_runtime_handler))
        .route("/api/channels/{kind}/stop", post(api::stop_channel_handler))
        .route(
            "/api/channels/{kind}/restart",
            post(api::restart_channel_handler),
        )
        .route(
            "/api/channels/{kind}/start",
            post(api::start_channel_handler),
        )
        .route("/api/tunnels/{provider}", delete(api::kill_tunnel_handler))
        .route("/api/agents/{route_key}", delete(api::kill_agent_handler))
        .route("/api/pty/{session_id}", delete(api::kill_pty_handler))
        .route("/api/previews", get(api::list_previews_handler))
        .route("/api/previews/{slug}", delete(api::delete_preview_handler))
        .route(
            "/api/workspaces",
            get(api::list_workspaces_handler).post(api::add_workspace_handler),
        )
        .route(
            "/api/workspaces/create",
            post(api::create_workspace_handler),
        )
        .route(
            "/api/workspaces/remove",
            post(api::remove_workspace_handler),
        )
        .route("/mcp", post(mcp::mcp_handler))
        .route_layer(axum::middleware::from_fn_with_state(
            auth_state.clone(),
            require_auth,
        ));

    // --- Open routes: SPA shell, static assets, and preview routes. ---------
    //
    // The SPA shell + static assets are intentionally un-authed so the initial
    // page load can boot and read the `?token=` parameter from its own URL.
    //
    // Preview routes are also un-authed — the 8-char slug itself acts as a
    // short-lived authentication token (10-min TTL, cryptographically random;
    // single source of truth: `common::previews::SHARE_TTL_SECS`).
    let bridge_routes = Router::new()
        .route(
            "/bridge/{profile_id}/{target_api_type}/v1/responses",
            post(api_bridge::legacy_responses_handler),
        )
        .route(
            "/bridge/{profile_id}/{target_api_type}/v1/chat/completions",
            post(api_bridge::legacy_chat_completions_handler),
        )
        .route(
            "/bridge/{profile_id}/{target_api_type}/v1/messages",
            post(api_bridge::legacy_messages_handler),
        )
        .route(
            "/bridge/{profile_id}/{target_api_type}/v1/models",
            get(api_bridge::legacy_models_handler),
        )
        .route(
            "/bridge/{profile_id}/{target_api_type}/{version}/models/{model_action}",
            post(api_bridge::legacy_gemini_generate_content_handler),
        )
        // Stable local API base for configured clients. `scope` selects the
        // route/profile preference; it is not a bridge session identifier.
        .route(
            "/local-api/{profile_id}/{scope}/{target_api_type}/v1/responses",
            post(api_bridge::local_responses_handler),
        )
        .route(
            "/local-api/{profile_id}/{scope}/{target_api_type}/v1/chat/completions",
            post(api_bridge::local_chat_completions_handler),
        )
        .route(
            "/local-api/{profile_id}/{scope}/{target_api_type}/v1/messages",
            post(api_bridge::local_messages_handler),
        )
        .route(
            "/local-api/{profile_id}/{scope}/{target_api_type}/v1/models",
            get(api_bridge::local_models_handler),
        )
        .route(
            "/local-api/{profile_id}/{scope}/{target_api_type}/{version}/models/{model_action}",
            post(api_bridge::local_gemini_generate_content_handler),
        )
        .layer(DefaultBodyLimit::max(LOCAL_BRIDGE_BODY_LIMIT_BYTES));

    let public = Router::new()
        .merge(bridge_routes)
        .route(
            "/internal/agent-hooks/codex",
            post(agent_hooks::codex_hook_handler),
        )
        // Pairing API: no auth required (pairing IS the auth flow).
        .route("/api/pair/start", post(pair::start_handler))
        .route("/api/pair/status", get(pair::status_handler))
        // Preview pages dispatch by session target:
        //   Server → iframe + `/`-scoped cookie proxy
        //   File   → rendered markdown page
        // /u = owner (requires va_owner cookie), /s = share (slug is auth).
        .route("/preview/u/{slug}", get(preview::owner_preview_handler))
        .route("/preview/s/{slug}", get(preview::share_preview_handler))
        // Legacy markdown route (kept for backward compatibility).
        .route("/md-preview/{slug}", get(preview::md_preview_handler))
        .nest_service("/assets", ServeDir::new(assets_dir))
        .nest_service("/brand", ServeDir::new(brand_dir))
        .fallback(any(spa_fallback_handler));

    // ALL VibeAround routes live under `/va/` — the root `/` namespace is
    // reserved exclusively for the cookie-based dev-server preview proxy.
    let dashboard = Router::new().merge(protected).merge(public);

    let app = Router::new()
        .nest("/va", dashboard)
        // Root fallback: cookie → proxy to dev server, else → /va/.
        .fallback(any(preview::cookie_proxy_fallback))
        .with_state(state)
        .layer(build_cors_layer(port));

    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::AddrInUse {
            tracing::info!(
                "[VibeAround] ⚠️  Port {} is already in use — is another VibeAround instance running?",
                port
            );
        }
        e
    })?;
    println!(
        "[VibeAround] Web server listening on http://127.0.0.1:{}",
        port
    );
    axum::serve(listener, app).await?;
    Ok(())
}

/// Build a tight CORS layer.
///
/// Allowed origins:
/// - `http://127.0.0.1:{port}` / `http://localhost:{port}` — the SPA served
///   from this same server, opened in a regular browser.
/// - `tauri://localhost` / `http://tauri.localhost` — the Tauri webview
///   on macOS/Linux and Windows respectively.
/// - `http://localhost:5181` — the desktop-ui Vite dev server during
///   development.
///
/// Everything else is rejected, so random websites the user visits cannot
/// fetch from the loopback port.
fn build_cors_layer(port: u16) -> tower_http::cors::CorsLayer {
    let origins: Vec<HeaderValue> = [
        format!("http://127.0.0.1:{port}"),
        format!("http://localhost:{port}"),
        "tauri://localhost".to_string(),
        "http://tauri.localhost".to_string(),
        "http://localhost:5181".to_string(),
    ]
    .into_iter()
    .filter_map(|s| HeaderValue::from_str(&s).ok())
    .collect();

    tower_http::cors::CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            axum::http::header::AUTHORIZATION,
            axum::http::header::CONTENT_TYPE,
            axum::http::header::ACCEPT,
            // `bypass-tunnel-reminder` is set by the SPA for loca.lt tunnels.
            axum::http::HeaderName::from_static("bypass-tunnel-reminder"),
        ])
}

#[cfg(test)]
mod tests {
    use super::is_dashboard_api_path;

    #[test]
    fn recognizes_dashboard_api_fallback_paths() {
        assert!(is_dashboard_api_path(
            "/va/local-api/deepseek/scope/extra/openai-chat/v1/responses"
        ));
        assert!(is_dashboard_api_path(
            "/local-api/deepseek/scope/extra/openai-chat/v1/responses"
        ));
        assert!(is_dashboard_api_path(
            "/va/bridge/profile/openai-chat/v1/responses"
        ));
        assert!(!is_dashboard_api_path("/va/"));
        assert!(!is_dashboard_api_path("/va/assets/index.css"));
    }
}
