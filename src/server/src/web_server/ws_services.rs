//! WebSocket handler for real-time service status push.
//!
//! - GET /ws/services — subscribe to service status changes

use axum::extract::{
    ws::{Message, WebSocket, WebSocketUpgrade},
    State,
};
use axum::response::Response;
use std::sync::Arc;

use super::AppState;

/// WebSocket upgrade handler for service status streaming.
pub async fn ws_services_handler(
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| ws_services_session(socket, state.services))
}

/// Stream service status snapshots to the client whenever changes occur.
async fn ws_services_session(
    mut socket: WebSocket,
    services: Arc<common::service::ServiceStatusManager>,
) {
    // 1. Send initial snapshot immediately
    let snapshot = services.snapshot();
    if let Ok(json) = serde_json::to_string(&snapshot) {
        if socket.send(Message::Text(json.into())).await.is_err() {
            return;
        }
    }

    // 2. Subscribe to changes and forward
    let mut rx = services.subscribe_changes();
    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(()) => {
                        let snapshot = services.snapshot();
                        if let Ok(json) = serde_json::to_string(&snapshot) {
                            if socket.send(Message::Text(json.into())).await.is_err() {
                                break; // client disconnected
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        eprintln!("[VibeAround][ws/services] lagged by {}, sending fresh snapshot", n);
                        let snapshot = services.snapshot();
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
