use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use va_client::endpoint::ServerEndpoint;
use va_client::events::{
    chat_ws_for_channel, decode_chat_event, encode_chat_client_message, ChatClientMessage,
    ChatEvent,
};

const CHAT_RECONNECT_MAX_DELAY: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub(crate) enum ChatSocketEvent {
    Connected,
    Closed,
    Error(String),
    Event(ChatEvent),
}

pub(crate) async fn run_chat_socket(
    endpoint: ServerEndpoint,
    mut outgoing: mpsc::UnboundedReceiver<ChatClientMessage>,
    incoming: mpsc::UnboundedSender<ChatSocketEvent>,
) {
    let url = endpoint.websocket_url(&chat_ws_for_channel("tui"));
    let mut failed_attempts = 0;

    loop {
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
                tokio::time::sleep(chat_reconnect_delay(failed_attempts)).await;
                continue;
            }
        };
        if incoming.send(ChatSocketEvent::Connected).is_err() {
            return;
        }
        let (mut ws_tx, mut ws_rx) = ws.split();

        loop {
            tokio::select! {
                message = outgoing.recv() => {
                    let Some(message) = message else {
                        let _ = ws_tx.close().await;
                        return;
                    };
                    let body = match encode_chat_client_message(&message) {
                        Ok(body) => body,
                        Err(error) => {
                            if incoming
                                .send(ChatSocketEvent::Error(format!("failed to encode chat message: {error}")))
                                .is_err()
                            {
                                return;
                            }
                            continue;
                        }
                    };
                    if let Err(error) = ws_tx.send(Message::Text(body.into())).await {
                        if incoming
                            .send(ChatSocketEvent::Error(format!("failed to send chat message: {error}")))
                            .is_err()
                        {
                            return;
                        }
                        break;
                    }
                }
                frame = ws_rx.next() => {
                    match frame {
                        Some(Ok(Message::Text(text))) => {
                            match serde_json::from_str::<Value>(&text) {
                                Ok(value) => match decode_chat_event(value) {
                                    Ok(event) => {
                                        if incoming.send(ChatSocketEvent::Event(event)).is_err() {
                                            return;
                                        }
                                    }
                                    Err(error) => {
                                        if incoming
                                            .send(ChatSocketEvent::Error(format!("failed to decode chat event: {error}")))
                                            .is_err()
                                        {
                                            return;
                                        }
                                    }
                                }
                                Err(error) => {
                                    if incoming
                                        .send(ChatSocketEvent::Error(format!("failed to parse chat event: {error}")))
                                        .is_err()
                                    {
                                        return;
                                    }
                                }
                            }
                        }
                        Some(Ok(Message::Close(_))) | None => {
                            if incoming.send(ChatSocketEvent::Closed).is_err() {
                                return;
                            }
                            break;
                        }
                        Some(Ok(_)) => {}
                        Some(Err(error)) => {
                            if incoming
                                .send(ChatSocketEvent::Error(format!("chat websocket read failed: {error}")))
                                .is_err()
                            {
                                return;
                            }
                            break;
                        }
                    }
                },
                else => break,
            };
        }

        let _ = ws_tx.close().await;
        failed_attempts += 1;
        tokio::time::sleep(chat_reconnect_delay(failed_attempts)).await;
    }
}

fn chat_reconnect_delay(failed_attempts: u32) -> Duration {
    let multiplier = 1_u64 << failed_attempts.saturating_sub(1).min(3);
    Duration::from_secs(multiplier).min(CHAT_RECONNECT_MAX_DELAY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_reconnect_delay_backs_off_and_caps() {
        assert_eq!(chat_reconnect_delay(0), Duration::from_secs(1));
        assert_eq!(chat_reconnect_delay(1), Duration::from_secs(1));
        assert_eq!(chat_reconnect_delay(2), Duration::from_secs(2));
        assert_eq!(chat_reconnect_delay(3), Duration::from_secs(4));
        assert_eq!(chat_reconnect_delay(4), CHAT_RECONNECT_MAX_DELAY);
        assert_eq!(chat_reconnect_delay(20), CHAT_RECONNECT_MAX_DELAY);
    }
}
