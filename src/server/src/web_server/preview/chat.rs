//! Owner-only chat socket for one Preview conversation.

use axum::extract::{
    ws::{Message, WebSocket, WebSocketUpgrade},
    Path, Query, Request, State,
};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use uuid::Uuid;

use common::channels::ChannelOutput;
use common::routing::RouteKey;
use common::workspace::manager::preview_web_route_for_slug;
use common::workspace::threads::WorkspaceThreadId;

use crate::api_types::ChatEvent;
use crate::web_server::ws_chat::{
    output_to_chat_event, parse_bound_chat_input, permission_response_error_event,
    remember_web_route_agent, remember_web_user_message, respond_to_web_permission, send_event,
    BoundChatInput,
};
use crate::web_server::AppState;

use super::owner_access_allowed;

#[derive(Debug, Default, Deserialize)]
pub(in crate::web_server) struct OwnerPreviewChatQuery {
    thread_id: Option<String>,
}

pub(in crate::web_server) async fn owner_preview_chat_handler(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<OwnerPreviewChatQuery>,
    ws: WebSocketUpgrade,
    req: Request,
) -> Response {
    let tunnel_urls = state.tunnels.public_urls();
    let route = match resolve_owner_chat_route(&slug, &req, state.port, &tunnel_urls) {
        Ok(route) => route,
        Err(error) => return error.into_response(),
    };
    let initial_thread_id =
        match resolve_owner_chat_thread_id(&state, &route, &slug, query.thread_id.as_deref()).await
        {
            Ok(thread_id) => thread_id,
            Err(error) => return error.into_response(),
        };

    ws.on_upgrade(move |socket| handle_owner_chat_socket(socket, state, route, initial_thread_id))
}

fn resolve_owner_chat_route(
    slug: &str,
    req: &Request,
    port: u16,
    tunnel_urls: &[String],
) -> Result<RouteKey, (StatusCode, &'static str)> {
    if !crate::web_server::auth::headers_have_allowed_ws_origin(req.headers(), port, tunnel_urls) {
        return Err((StatusCode::FORBIDDEN, "Preview chat origin is not allowed."));
    }
    if !owner_access_allowed(req) {
        return Err((
            StatusCode::UNAUTHORIZED,
            "Preview owner access is required.",
        ));
    }

    common::previews::lookup_owner(slug)
        .ok_or((StatusCode::NOT_FOUND, "Preview not found or expired."))?;
    Ok(preview_web_route_for_slug(slug))
}

async fn resolve_owner_chat_thread_id(
    state: &AppState,
    route: &RouteKey,
    slug: &str,
    thread_hint: Option<&str>,
) -> Result<WorkspaceThreadId, (StatusCode, &'static str)> {
    let manager = state.channel_hub.workspace_thread_manager();
    if let Some(runtime) = manager
        .active_route_runtime(route)
        .await
        .map_err(|_| preview_conversation_unavailable())?
    {
        return Ok(runtime.state().await.thread_id);
    }
    let hinted_runtime = match thread_hint.map(str::trim).filter(|hint| !hint.is_empty()) {
        Some(hint) => manager
            .attach_preview_web_thread_hint(slug, &WorkspaceThreadId::from(hint))
            .await
            .map_err(|_| preview_conversation_unavailable())?,
        None => None,
    };
    let runtime = match hinted_runtime {
        Some(runtime) => runtime,
        None => manager
            .ensure_preview_web_thread(None, slug)
            .await
            .map_err(|_| preview_conversation_unavailable())?,
    };
    Ok(runtime.state().await.thread_id)
}

fn preview_conversation_unavailable() -> (StatusCode, &'static str) {
    (
        StatusCode::CONFLICT,
        "Preview conversation is unavailable. Recreate this Preview from the current task.",
    )
}

async fn handle_owner_chat_socket(
    socket: WebSocket,
    state: AppState,
    route: RouteKey,
    initial_thread_id: WorkspaceThreadId,
) {
    let connection_id = Uuid::new_v4().to_string();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    state
        .web_channel
        .register_connection(&route, connection_id.clone(), tx, true)
        .await;
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<ChatEvent>();
    let (mut ws_tx, mut ws_rx) = socket.split();
    let outbound_state = state.clone();
    let outbound_route = route.clone();

    let outbound_task = tokio::spawn(async move {
        let mut announced_thread_id = initial_thread_id;
        if send_preview_conversation(&mut ws_tx, &announced_thread_id)
            .await
            .is_err()
        {
            return;
        }
        loop {
            tokio::select! {
                Some(output) = rx.recv() => {
                    if matches!(&output, ChannelOutput::SessionReady { .. }) {
                        if let Some(thread_id) = active_preview_thread_id(&outbound_state, &outbound_route).await {
                            if thread_id != announced_thread_id {
                                if send_preview_conversation(&mut ws_tx, &thread_id).await.is_err() {
                                    break;
                                }
                                announced_thread_id = thread_id;
                            }
                        }
                    }
                    if send_event(&mut ws_tx, &output_to_chat_event(output)).await.is_err() {
                        break;
                    }
                }
                Some(event) = event_rx.recv() => {
                    if send_event(&mut ws_tx, &event).await.is_err() {
                        break;
                    }
                }
                else => break,
            }
        }
    });

    while let Some(Ok(message)) = ws_rx.next().await {
        match message {
            Message::Text(text) => {
                let Some(input) = parse_bound_chat_input(&route, "preview-owner", &text) else {
                    continue;
                };
                match input {
                    BoundChatInput::Message(input) => {
                        // SessionReady can bind and replay this user message only
                        // after WebChannelManager knows the route's current agent.
                        remember_web_route_agent(&state, &route, None).await;
                        let wait_for_session_ready =
                            preview_user_message_waits_for_session(&state, &route).await;
                        remember_web_user_message(&state, &route, &input, wait_for_session_ready)
                            .await;
                        state.channel_hub.handle_input(input);
                    }
                    BoundChatInput::Stop(input) => state.channel_hub.handle_input(input),
                    BoundChatInput::PermissionResponse {
                        request_id,
                        response,
                    } => {
                        if let Err(error) =
                            respond_to_web_permission(&state, &route, &request_id, response).await
                        {
                            tracing::warn!(
                                request_id = %request_id,
                                error = %error,
                                "Preview permission response ignored"
                            );
                            let _ =
                                event_tx.send(permission_response_error_event(&request_id, &error));
                        }
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    outbound_task.abort();
    // The route belongs to the Preview conversation, not to this browser
    // connection. Preserve its thread attachment and replay state.
    state
        .web_channel
        .unregister_connection(&route, &connection_id)
        .await;
}

async fn active_preview_thread_id(state: &AppState, route: &RouteKey) -> Option<WorkspaceThreadId> {
    let runtime = state
        .channel_hub
        .workspace_thread_manager()
        .active_route_runtime(route)
        .await
        .ok()??;
    Some(runtime.state().await.thread_id)
}

fn preview_conversation_frame(thread_id: &WorkspaceThreadId) -> String {
    serde_json::json!({
        "kind": "preview_conversation",
        "thread_id": thread_id.as_str(),
    })
    .to_string()
}

async fn send_preview_conversation<S>(
    ws_tx: &mut S,
    thread_id: &WorkspaceThreadId,
) -> Result<(), ()>
where
    S: SinkExt<Message, Error = axum::Error> + Unpin,
{
    ws_tx
        .send(Message::Text(preview_conversation_frame(thread_id).into()))
        .await
        .map_err(|_| ())
}

async fn preview_user_message_waits_for_session(state: &AppState, route: &RouteKey) -> bool {
    let Some(route_session) = state.web_channel.route_session_id(route).await else {
        return false;
    };
    let active_session = match state
        .channel_hub
        .workspace_thread_manager()
        .active_route_runtime(route)
        .await
    {
        Ok(Some(runtime)) => runtime.state().await.session_id,
        _ => None,
    };
    preview_route_session_is_stale(Some(&route_session), active_session.as_deref())
}

fn preview_route_session_is_stale(
    route_session: Option<&str>,
    active_session: Option<&str>,
) -> bool {
    route_session.is_some() && route_session != active_session
}

#[cfg(test)]
#[path = "chat_tests.rs"]
mod tests;
