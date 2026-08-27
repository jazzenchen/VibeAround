use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use va_client::events::{
    chat_ws_for_channel, decode_chat_event, encode_chat_client_message, ChatClientMessage,
    ChatEvent,
};

use crate::socket_retry::{socket_retry_after_failure, SocketRetry, SOCKET_RETRY_INTERVAL};
use crate::transport::SharedEndpoint;

#[derive(Debug, PartialEq)]
pub(crate) enum ChatSocketEvent {
    Connected,
    Closed,
    /// Terminal state: the loop gave up retrying and is parked until the app
    /// signals a reconnect.
    Disconnected,
    Error(String),
    Event(ChatEvent),
}

#[derive(Debug, PartialEq)]
enum ChatSocketAction {
    Send(ChatClientMessage),
    Continue,
    Reconnect,
    Stop,
}

pub(crate) async fn run_chat_socket(
    endpoint: Arc<SharedEndpoint>,
    mut outgoing: mpsc::UnboundedReceiver<ChatClientMessage>,
    incoming: mpsc::UnboundedSender<ChatSocketEvent>,
    mut reconnect: watch::Receiver<()>,
) {
    let socket = chat_ws_for_channel("tui");
    let mut failed_attempts = 0;
    let mut pending_message = None;

    loop {
        // A daemon restart rotates the token, so re-read the auth file and
        // recompute the URL before every attempt.
        endpoint.refresh_token();
        let url = endpoint.websocket_url(&socket);
        let (ws, _) = match connect_async(&url).await {
            Ok(connection) => {
                failed_attempts = 0;
                connection
            }
            Err(error) => {
                failed_attempts += 1;
                if incoming
                    .send(ChatSocketEvent::Error(format!(
                        "failed to connect chat websocket: {error}"
                    )))
                    .is_err()
                {
                    return;
                }
                match socket_retry_after_failure(failed_attempts) {
                    SocketRetry::RetryAfter(delay) => tokio::time::sleep(delay).await,
                    SocketRetry::GiveUp => {
                        // Only wake for a reconnect requested after the app
                        // has seen the terminal state.
                        reconnect.mark_unchanged();
                        if incoming.send(ChatSocketEvent::Disconnected).is_err() {
                            return;
                        }
                        if reconnect.changed().await.is_err() {
                            return;
                        }
                        failed_attempts = 0;
                    }
                }
                continue;
            }
        };
        if incoming.send(ChatSocketEvent::Connected).is_err() {
            return;
        }
        let (mut ws_tx, mut ws_rx) = ws.split();

        loop {
            let action = if let Some(message) = pending_message.take() {
                ChatSocketAction::Send(message)
            } else {
                tokio::select! {
                    message = outgoing.recv() => {
                    let Some(message) = message else {
                        let _ = ws_tx.close().await;
                        return;
                    };
                        ChatSocketAction::Send(message)
                    }
                    frame = ws_rx.next() => chat_socket_action_for_frame(frame, &incoming),
                    else => ChatSocketAction::Reconnect,
                }
            };

            match action {
                ChatSocketAction::Send(message) => {
                    let body = match encode_chat_client_message(&message) {
                        Ok(body) => body,
                        Err(error) => {
                            if incoming
                                .send(ChatSocketEvent::Error(format!(
                                    "failed to encode chat message: {error}"
                                )))
                                .is_err()
                            {
                                return;
                            }
                            continue;
                        }
                    };
                    if let Err(error) = ws_tx.send(Message::Text(body.into())).await {
                        if incoming
                            .send(ChatSocketEvent::Error(format!(
                                "failed to send chat message: {error}"
                            )))
                            .is_err()
                        {
                            return;
                        }
                        pending_message = Some(message);
                        break;
                    }
                }
                ChatSocketAction::Continue => {}
                ChatSocketAction::Reconnect => break,
                ChatSocketAction::Stop => return,
            }
        }

        // The connection itself succeeded, so a drop starts a fresh retry
        // cycle rather than counting toward the failure limit.
        let _ = ws_tx.close().await;
        tokio::time::sleep(SOCKET_RETRY_INTERVAL).await;
    }
}

fn chat_socket_action_for_frame(
    frame: Option<Result<Message, tokio_tungstenite::tungstenite::Error>>,
    incoming: &mpsc::UnboundedSender<ChatSocketEvent>,
) -> ChatSocketAction {
    match frame {
        Some(Ok(Message::Text(text))) => {
            match serde_json::from_str::<Value>(&text) {
                Ok(value) => match decode_chat_event(value) {
                    Ok(event) => {
                        if incoming.send(ChatSocketEvent::Event(event)).is_err() {
                            return ChatSocketAction::Stop;
                        }
                    }
                    Err(error) => {
                        if incoming
                            .send(ChatSocketEvent::Error(format!(
                                "failed to decode chat event: {error}"
                            )))
                            .is_err()
                        {
                            return ChatSocketAction::Stop;
                        }
                    }
                },
                Err(error) => {
                    if incoming
                        .send(ChatSocketEvent::Error(format!(
                            "failed to parse chat event: {error}"
                        )))
                        .is_err()
                    {
                        return ChatSocketAction::Stop;
                    }
                }
            }
            ChatSocketAction::Continue
        }
        Some(Ok(Message::Close(_))) | None => {
            if incoming.send(ChatSocketEvent::Closed).is_err() {
                return ChatSocketAction::Stop;
            }
            ChatSocketAction::Reconnect
        }
        Some(Ok(_)) => ChatSocketAction::Continue,
        Some(Err(error)) => {
            if incoming
                .send(ChatSocketEvent::Error(format!(
                    "chat websocket read failed: {error}"
                )))
                .is_err()
            {
                return ChatSocketAction::Stop;
            }
            ChatSocketAction::Reconnect
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_chat_frame_requests_reconnect() {
        let (tx, mut rx) = mpsc::unbounded_channel();

        let action = chat_socket_action_for_frame(None, &tx);

        assert_eq!(action, ChatSocketAction::Reconnect);
        assert_eq!(
            rx.try_recv().expect("closed event"),
            ChatSocketEvent::Closed
        );
    }
}
