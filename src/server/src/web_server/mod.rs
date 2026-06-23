//! Axum HTTP + WebSocket server: serves Web SPA (from given dist path), WS at /ws for xterm ↔ PTY,
//! agent chat WS at /ws/chat, live preview (/preview/:slug with iframe wrapper + reverse proxy),
//! and MCP endpoint at /mcp.

mod api;
mod api_bridge;
mod auth;
mod bridge_recording;
mod mcp;
mod pair;
mod preview;
mod ws_chat;
mod ws_domains;
mod ws_pty;

use axum::body::Body;
use axum::extract::DefaultBodyLimit;
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, delete, get, post, put};
use axum::Router;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Notify;
use tower_http::services::ServeDir;

use common::auth::AuthToken;
use common::channels::{ChannelManager, WebChannelManager};
use common::pty::{PtySessionManager, Registry};
use common::search::SearchToolRuntime;
use common::tunnels::TunnelManager;

use self::auth::{require_auth, require_local_bridge, AuthState};

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
    /// True when settings enable at least one host-side search source.
    host_search_available: bool,
    /// Replace native provider web search with host-side search even when the
    /// selected upstream model declares native web search support.
    replace_provider_web_search: bool,
    search_runtime: Option<Arc<SearchToolRuntime>>,
    /// Live, non-persistent bridge body recorder for the launch popup.
    bridge_recorder: bridge_recording::BridgeRecorder,
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
    [
        "/va/local-api/",
        "/local-api/",
        "/va/local-agent/",
        "/local-agent/",
        "/va/bridge/",
        "/bridge/",
    ]
    .into_iter()
    .any(|prefix| path.starts_with(prefix))
}

pub(crate) async fn bind_web_listener(port: u16) -> std::io::Result<tokio::net::TcpListener> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    clear_inherit_flag(&listener)?;
    Ok(listener)
}

#[cfg(windows)]
fn clear_inherit_flag(listener: &tokio::net::TcpListener) -> std::io::Result<()> {
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT};

    let handle = listener.as_raw_socket() as usize as HANDLE;
    let ok = unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(windows))]
fn clear_inherit_flag(_: &tokio::net::TcpListener) -> std::io::Result<()> {
    Ok(())
}

/// Runs the Axum server (static files + WebSocket + session API). The listener is bound to
/// 127.0.0.1 by ServerDaemon before startup returns.
/// Call from desktop via tauri::async_runtime::spawn, or run standalone via the server binary.
pub async fn run_web_server(
    listener: tokio::net::TcpListener,
    dist_path: PathBuf,
    tunnels: Arc<TunnelManager>,
    pty_registry: Registry,
    channel_hub: Arc<ChannelManager>,
    web_channel: Arc<WebChannelManager>,
    auth_token: Arc<AuthToken>,
    host_search_available: bool,
    replace_provider_web_search: bool,
    search_runtime: Option<Arc<SearchToolRuntime>>,
    shutdown: Arc<Notify>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    verify_web_dist(&dist_path)?;
    let web_dist = dist_path
        .canonicalize()
        .map_err(|e| format!("Failed to resolve web dist path: {}", e))?;
    let port = listener.local_addr()?.port();
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
        host_search_available,
        replace_provider_web_search,
        search_runtime,
        bridge_recorder: bridge_recording::BridgeRecorder::default(),
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
        .route("/api/service/info", get(api::info_handler))
        .route(
            "/api/settings",
            get(api::get_settings_handler).put(api::put_settings_handler),
        )
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
        .route(
            "/api/model-profiles",
            get(api::list_model_profiles_handler).post(api::create_model_profile_handler),
        )
        .route(
            "/api/model-profiles/order",
            put(api::reorder_model_profiles_handler),
        )
        .route(
            "/api/model-profiles/{id}",
            get(api::get_model_profile_handler)
                .put(api::update_model_profile_handler)
                .delete(api::delete_model_profile_handler),
        )
        .route(
            "/api/launcher/preferences",
            get(api::get_launcher_preferences_handler),
        )
        .route(
            "/api/launcher/default-agent",
            put(api::set_default_launch_handler),
        )
        .route(
            "/api/launcher/agent-profile",
            put(api::set_agent_profile_handler),
        )
        .route(
            "/api/launcher/agent-workspace",
            put(api::set_agent_workspace_handler),
        )
        .route(
            "/api/launcher/agent-launch-args",
            put(api::set_agent_launch_args_handler),
        )
        .route(
            "/api/launcher/selected-agent",
            put(api::set_selected_agent_handler),
        )
        .route(
            "/api/launcher/local-agent-api",
            put(api::set_local_agent_api_handler),
        )
        .route(
            "/api/launcher/profile-connection",
            put(api::set_profile_connection_handler),
        )
        .route("/api/launcher/plan", post(api::launcher_plan_handler))
        .route("/ws", get(ws_pty::ws_handler))
        .route("/ws/chat", get(ws_chat::ws_chat_handler))
        .route("/ws/channels", get(ws_domains::ws_channels_handler))
        .route("/ws/tunnels", get(ws_domains::ws_tunnels_handler))
        .route(
            "/ws/agents/runtime",
            get(ws_domains::ws_agents_runtime_handler),
        )
        .route(
            "/ws/bridge-recording",
            get(bridge_recording::bridge_recording_ws_handler),
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
        .route(
            "/api/workspaces/order",
            put(api::reorder_workspaces_handler),
        )
        .route(
            "/api/workspaces/default",
            put(api::set_default_workspace_handler),
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
            "/local-agent/{agent_id}/{profile_id}/v1/responses",
            post(api_bridge::local_agent_responses_handler),
        )
        .route(
            "/local-agent/{agent_id}/{profile_id}/v1/chat/completions",
            post(api_bridge::local_agent_chat_completions_handler),
        )
        .route(
            "/local-agent/{agent_id}/{profile_id}/v1/messages",
            post(api_bridge::local_agent_messages_handler),
        )
        .route(
            "/local-agent/{agent_id}/{profile_id}/v1/models",
            get(api_bridge::local_agent_models_handler),
        )
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
        .route_layer(axum::middleware::from_fn(require_local_bridge))
        .layer(DefaultBodyLimit::max(LOCAL_BRIDGE_BODY_LIMIT_BYTES));

    let public = Router::new()
        .merge(bridge_routes)
        .route("/api/service/health", get(api::health_handler))
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

    println!(
        "[VibeAround] Web server listening on http://127.0.0.1:{}",
        port
    );
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown.notified().await;
    })
    .await?;
    Ok(())
}

/// Build an open CORS layer for the local daemon API.
///
/// VibeAround intentionally exposes local API-compatible endpoints for tools,
/// scripts, and browser clients running on the user's machine. Authentication
/// still protects dashboard APIs; CORS should not be the capability boundary.
fn build_cors_layer(_port: u16) -> tower_http::cors::CorsLayer {
    tower_http::cors::CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(tower_http::cors::Any)
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
            "/va/local-agent/claude/direct/v1/responses"
        ));
        assert!(is_dashboard_api_path(
            "/local-api/deepseek/scope/extra/openai-chat/v1/responses"
        ));
        assert!(is_dashboard_api_path(
            "/local-agent/claude/direct/v1/responses"
        ));
        assert!(is_dashboard_api_path(
            "/va/bridge/profile/openai-chat/v1/responses"
        ));
        assert!(!is_dashboard_api_path("/va/"));
        assert!(!is_dashboard_api_path("/va/assets/index.css"));
    }
}
