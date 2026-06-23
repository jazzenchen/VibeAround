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
    let (ws, _) = match connect_async(&url).await {
        Ok(connection) => connection,
        Err(error) => {
            let _ = incoming.send(ChatSocketEvent::Error(format!(
                "failed to connect chat websocket: {error}"
            )));
            return;
        }
    };
    let _ = incoming.send(ChatSocketEvent::Connected);
    let (mut ws_tx, mut ws_rx) = ws.split();

    loop {
        tokio::select! {
            Some(message) = outgoing.recv() => {
                let body = match encode_chat_client_message(&message) {
                    Ok(body) => body,
                    Err(error) => {
                        let _ = incoming.send(ChatSocketEvent::Error(format!("failed to encode chat message: {error}")));
                        continue;
                    }
                };
                if let Err(error) = ws_tx.send(Message::Text(body.into())).await {
                    let _ = incoming.send(ChatSocketEvent::Error(format!("failed to send chat message: {error}")));
                    break;
                }
            }
            frame = ws_rx.next() => {
                match frame {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<Value>(&text) {
                            Ok(value) => match decode_chat_event(value) {
                                Ok(event) => {
                                    let _ = incoming.send(ChatSocketEvent::Event(event));
                                }
                                Err(error) => {
                                    let _ = incoming.send(ChatSocketEvent::Error(format!("failed to decode chat event: {error}")));
                                }
                            },
                            Err(error) => {
                                let _ = incoming.send(ChatSocketEvent::Error(format!("failed to parse chat event: {error}")));
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        let _ = incoming.send(ChatSocketEvent::Closed);
                        break;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        let _ = incoming.send(ChatSocketEvent::Error(format!("chat websocket read failed: {error}")));
                        break;
                    }
                }
            }
            else => break,
        }
    }

    let _ = ws_tx.close().await;
}
