//! Per-domain WebSocket handlers.
//!
//! Each endpoint mirrors the corresponding `GET /api/<domain>` HTTP
//! handler: on connect it immediately sends the current list, then
//! subscribes to the manager's `subscribe_changes()` receiver and
//! re-sends the list on every `()` ping. Client-side the usage is
//! "whatever you last received IS the state; replace your view on
//! each message".
//!
//! Factored behind a tiny generic helper (`run_ws_loop`) so the three
//! endpoints only differ in which manager they poll and which wire
//! shape they produce.

use std::future::Future;

use axum::extract::{
    ws::{Message, WebSocket, WebSocketUpgrade},
    State,
};
use axum::response::Response;
use common::state::StateSource;
use serde::Serialize;
use tokio::sync::broadcast;

use super::AppState;

// ---------------------------------------------------------------------------
// GET /ws/channels
// ---------------------------------------------------------------------------

pub async fn ws_channels_handler(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| async move {
        let monitor = state.channel_hub.monitor();
        let rx = monitor.subscribe_changes();
        run_ws_loop(socket, rx, "ws/channels", move || {
            let monitor = monitor.clone();
            async move { build_channels(&monitor).await }
        })
        .await;
    })
}

async fn build_channels(
    monitor: &common::channels::monitor::ChannelMonitor,
) -> Vec<crate::api_types::ChannelRuntime> {
    use common::state::StateSource;
    monitor
        .list()
        .await
        .into_iter()
        .map(|s| crate::api_types::ChannelRuntime {
            kind: s.kind,
            status: s.status.as_str(),
            reason: if s.reason.is_empty() {
                None
            } else {
                Some(s.reason)
            },
            crash_count: s.crash_count,
            last_seen_age_secs: s.last_seen_age_secs,
            restart_in_secs: s.restart_in_secs,
            started_at: s.started_at,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// GET /ws/tunnels
// ---------------------------------------------------------------------------

pub async fn ws_tunnels_handler(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| async move {
        let tunnels = state.tunnels.clone();
        let rx = tunnels.subscribe_changes();
        run_ws_loop(socket, rx, "ws/tunnels", move || {
            let tunnels = tunnels.clone();
            async move { build_tunnels(&tunnels).await }
        })
        .await;
    })
}

async fn build_tunnels(
    manager: &common::tunnels::TunnelManager,
) -> Vec<crate::api_types::TunnelRuntime> {
    use common::state::StateSource;
    manager
        .list()
        .await
        .into_iter()
        .map(|t| crate::api_types::TunnelRuntime {
            provider: t.provider.as_str(),
            url: t.url,
            status: t.status,
            uptime_secs: t.uptime_secs,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// GET /ws/agents/runtime
// ---------------------------------------------------------------------------

pub async fn ws_agents_runtime_handler(
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| async move {
        let workspace_threads = state.channel_hub.workspace_thread_manager();
        let rx = workspace_threads.subscribe_changes();
        run_ws_loop(socket, rx, "ws/agents/runtime", move || {
            let workspace_threads = workspace_threads.clone();
            async move { build_agents_runtime(&workspace_threads).await }
        })
        .await;
    })
}

async fn build_agents_runtime(
    workspace_threads: &common::workspace::WorkspaceThreadManager,
) -> Vec<crate::api_types::AgentRuntime> {
    let entries = workspace_threads.list().await;
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        let st = entry.state;
        let (agent_name, agent_title, agent_version) = st
            .initialize
            .as_ref()
            .and_then(|i| i.agent_info.as_ref())
            .map(|info| {
                (
                    Some(info.name.clone()),
                    info.title.clone(),
                    Some(info.version.clone()),
                )
            })
            .unwrap_or((None, None, None));
        let (route_key, channel_kind, chat_id) = match entry.route {
            Some(route) => (
                route.as_key(),
                route.channel_kind.clone(),
                route.chat_id.clone(),
            ),
            None => (
                st.thread_id.to_string(),
                "workspace".to_string(),
                st.thread_id.to_string(),
            ),
        };
        out.push(crate::api_types::AgentRuntime {
            route_key,
            channel_kind,
            chat_id,
            cli_kind: Some(st.host_binding.agent_id.clone()),
            profile: st.host_binding.profile_id.clone(),
            session_id: st.session_id,
            workspace: Some(st.workspace.to_string_lossy().to_string()),
            busy: st.busy,
            failed: st.failed,
            started_at: 0,
            agent_name,
            agent_title,
            agent_version,
            multi_agent_turns: st.multi_agent_turns,
            subagents: st.agents,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Shared loop
// ---------------------------------------------------------------------------

/// Runs a WS session: first emit, then emit on every `()` ping from the
/// receiver. `build` produces the current list on demand; it's called
/// once at open and again on each change.
async fn run_ws_loop<T, F, Fut>(
    mut socket: WebSocket,
    mut rx: broadcast::Receiver<()>,
    label: &'static str,
    build: F,
) where
    T: Serialize,
    F: Fn() -> Fut + Send + Sync,
    Fut: Future<Output = T> + Send,
{
    if send_current(&mut socket, &build).await.is_err() {
        return;
    }
    loop {
        tokio::select! {
            result = rx.recv() => match result {
                Ok(()) => {
                    if send_current(&mut socket, &build).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::info!("[VibeAround][{}] lagged by {}, sending fresh", label, n);
                    if send_current(&mut socket, &build).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
            },
            msg = socket.recv() => match msg {
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(Message::Ping(data))) => {
                    let _ = socket.send(Message::Pong(data)).await;
                }
                _ => {}
            },
        }
    }
}

async fn send_current<T, F, Fut>(socket: &mut WebSocket, build: &F) -> Result<(), ()>
where
    T: Serialize,
    F: Fn() -> Fut,
    Fut: Future<Output = T>,
{
    let data = build().await;
    let json = match serde_json::to_string(&data) {
        Ok(j) => j,
        Err(_) => return Ok(()), // skip malformed frames, don't close
    };
    if socket.send(Message::Text(json.into())).await.is_err() {
        return Err(());
    }
    Ok(())
}
