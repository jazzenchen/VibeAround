//! WebSocket handler for PTY terminal sessions.
//!
//! - GET /ws?session_id=<uuid> — attach to an existing PTY session

use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, Query, State},
    response::Response,
};
use bytes::Bytes;
use futures_util::stream::StreamExt;
use futures_util::SinkExt;
use std::io::Write;

use common::pty::{PtySessionManager, SessionId};

use super::{AppState, WsQuery};

/// WebSocket upgrade handler for PTY sessions.
pub async fn ws_handler(
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    if let Some(ref sid) = query.session_id {
        if let Ok(uuid) = uuid::Uuid::parse_str(sid) {
            let session_id = SessionId(uuid);
            let pty_manager = state.pty_manager.clone();
            return ws.on_upgrade(move |socket| handle_socket_attach(socket, session_id, pty_manager));
        }
    }
    // session_id is required; reject bare /ws connections.
    ws.on_upgrade(|mut socket| async move {
        let _ = socket.send(Message::Text("Missing or invalid session_id".into())).await;
    })
}

/// Attach a WebSocket to an existing PTY session: replay buffer, then bridge live I/O.
async fn handle_socket_attach(
    mut socket: WebSocket,
    session_id: SessionId,
    pty_manager: std::sync::Arc<PtySessionManager>,
) {
    let Some(handles) = pty_manager.attach_handles(session_id) else {
        let _ = socket.send(Message::Text("Session not found".into())).await;
        return;
    };
    let buffer = handles.buffer;
    let state = handles.state;
    let live_tx = handles.live_tx;
    let writer = handles.writer;
    let resize_tx = handles.resize_tx;
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
                    if let Ok(resize) = serde_json::from_str::<super::ResizeMessage>(t) {
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
