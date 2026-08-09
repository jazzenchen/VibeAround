//! Owner-only chat socket for one Preview conversation.

use axum::extract::{
    ws::{Message, WebSocket, WebSocketUpgrade},
    Path, Request, State,
};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use uuid::Uuid;

use common::routing::RouteKey;
use common::workspace::manager::web_route_for_thread;

use crate::api_types::ChatEvent;
use crate::web_server::ws_chat::{
    output_to_chat_event, parse_bound_chat_input, permission_response_error_event,
    remember_web_route_agent, remember_web_user_message, respond_to_web_permission, send_event,
    BoundChatInput,
};
use crate::web_server::AppState;

use super::{owner_access_allowed, preview_target_available};

pub(in crate::web_server) async fn owner_preview_chat_handler(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    ws: WebSocketUpgrade,
    req: Request,
) -> Response {
    let tunnel_urls = state.tunnels.public_urls();
    let route = match resolve_owner_chat_route(&slug, &req, state.port, &tunnel_urls) {
        Ok(route) => route,
        Err(error) => return error.into_response(),
    };

    ws.on_upgrade(move |socket| handle_owner_chat_socket(socket, state, route))
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

    let entry = common::previews::lookup_owner(slug)
        .ok_or((StatusCode::NOT_FOUND, "Preview not found or expired."))?;
    if !preview_target_available(req, &entry) {
        return Err(super::server_preview_local_only());
    }
    let thread_id = common::previews::owner_conversation_thread_id(slug).ok_or((
        StatusCode::CONFLICT,
        "Preview conversation is unavailable. Recreate this Preview from the current task.",
    ))?;
    Ok(web_route_for_thread(&thread_id))
}

async fn handle_owner_chat_socket(socket: WebSocket, state: AppState, route: RouteKey) {
    let connection_id = Uuid::new_v4().to_string();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    state
        .web_channel
        .register_connection(&route, connection_id.clone(), tx, true)
        .await;
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<ChatEvent>();
    let (mut ws_tx, mut ws_rx) = socket.split();

    let outbound_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(output) = rx.recv() => {
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
                        remember_web_user_message(&state, &route, &input, false).await;
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

#[cfg(test)]
#[path = "chat_tests.rs"]
mod tests;
