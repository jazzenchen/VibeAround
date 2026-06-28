use std::time::Duration;

use futures_util::StreamExt;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use va_client::endpoint::ServerEndpoint;
use va_client::events::{
    agents_runtime_ws, channels_ws, decode_agents_runtime_event, decode_channels_event,
    decode_sessions_event, decode_tunnels_event, sessions_ws, tunnels_ws, WebSocketSpec,
};
use va_client::runtime::{AgentRuntime, ChannelRuntime, TunnelRuntime};
use va_client::sessions::SessionListItem;

const RUNTIME_RECONNECT_MAX_DELAY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeStream {
    Channels,
    Tunnels,
    Agents,
    Sessions,
}

#[derive(Debug)]
pub(crate) enum RuntimeSocketEvent {
    Channels(Vec<ChannelRuntime>),
    Tunnels(Vec<TunnelRuntime>),
    Agents(Vec<AgentRuntime>),
    Sessions(Vec<SessionListItem>),
    Error {
        stream: RuntimeStream,
        message: String,
    },
}

pub(crate) async fn run_runtime_sockets(
    endpoint: ServerEndpoint,
    incoming: mpsc::UnboundedSender<RuntimeSocketEvent>,
) {
    let tasks = [
        tokio::spawn(run_channels_socket(endpoint.clone(), incoming.clone())),
        tokio::spawn(run_tunnels_socket(endpoint.clone(), incoming.clone())),
        tokio::spawn(run_agents_socket(endpoint.clone(), incoming.clone())),
        tokio::spawn(run_sessions_socket(endpoint, incoming)),
    ];
    for task in tasks {
        let _ = task.await;
    }
}

async fn run_channels_socket(
    endpoint: ServerEndpoint,
    incoming: mpsc::UnboundedSender<RuntimeSocketEvent>,
) {
    run_snapshot_socket(
        endpoint,
        RuntimeStream::Channels,
        channels_ws(),
        incoming,
        |value| decode_channels_event(value).map(RuntimeSocketEvent::Channels),
    )
    .await;
}

async fn run_tunnels_socket(
    endpoint: ServerEndpoint,
    incoming: mpsc::UnboundedSender<RuntimeSocketEvent>,
) {
    run_snapshot_socket(
        endpoint,
        RuntimeStream::Tunnels,
        tunnels_ws(),
        incoming,
        |value| decode_tunnels_event(value).map(RuntimeSocketEvent::Tunnels),
    )
    .await;
}

async fn run_agents_socket(
    endpoint: ServerEndpoint,
    incoming: mpsc::UnboundedSender<RuntimeSocketEvent>,
) {
    run_snapshot_socket(
        endpoint,
        RuntimeStream::Agents,
        agents_runtime_ws(),
        incoming,
        |value| decode_agents_runtime_event(value).map(RuntimeSocketEvent::Agents),
    )
    .await;
}

async fn run_sessions_socket(
    endpoint: ServerEndpoint,
    incoming: mpsc::UnboundedSender<RuntimeSocketEvent>,
) {
    run_snapshot_socket(
        endpoint,
        RuntimeStream::Sessions,
        sessions_ws(),
        incoming,
        |value| decode_sessions_event(value).map(RuntimeSocketEvent::Sessions),
    )
    .await;
}

async fn run_snapshot_socket(
    endpoint: ServerEndpoint,
    stream: RuntimeStream,
    socket: WebSocketSpec,
    incoming: mpsc::UnboundedSender<RuntimeSocketEvent>,
    decode: fn(Value) -> va_client::Result<RuntimeSocketEvent>,
) {
    let label = socket.path.clone();
    let url = endpoint.websocket_url(&socket);
    let mut failed_attempts = 0;

    loop {
        let (mut ws, _) = match connect_async(&url).await {
            Ok(connection) => {
                failed_attempts = 0;
                connection
            }
            Err(error) => {
                failed_attempts += 1;
                if incoming
                    .send(RuntimeSocketEvent::Error {
                        stream,
                        message: format!("failed to connect {label}: {error}"),
                    })
                    .is_err()
                {
                    return;
                }
                tokio::time::sleep(runtime_reconnect_delay(failed_attempts)).await;
                continue;
            }
        };

        while let Some(frame) = ws.next().await {
            match frame {
                Ok(Message::Text(text)) => match serde_json::from_str::<Value>(&text) {
                    Ok(value) => match decode(value) {
                        Ok(event) => {
                            if incoming.send(event).is_err() {
                                return;
                            }
                        }
                        Err(error) => {
                            if incoming
                                .send(RuntimeSocketEvent::Error {
                                    stream,
                                    message: format!("failed to decode {label}: {error}"),
                                })
                                .is_err()
                            {
                                return;
                            }
                        }
                    },
                    Err(error) => {
                        if incoming
                            .send(RuntimeSocketEvent::Error {
                                stream,
                                message: format!("failed to parse {label}: {error}"),
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                },
                Ok(Message::Close(_)) => break,
                Ok(_) => {}
                Err(error) => {
                    if incoming
                        .send(RuntimeSocketEvent::Error {
                            stream,
                            message: format!("{label} websocket read failed: {error}"),
                        })
                        .is_err()
                    {
                        return;
                    }
                    break;
                }
            }
        }

        failed_attempts += 1;
        tokio::time::sleep(runtime_reconnect_delay(failed_attempts)).await;
    }
}

fn runtime_reconnect_delay(failed_attempts: u32) -> Duration {
    let multiplier = 1_u64 << failed_attempts.saturating_sub(1).min(3);
    Duration::from_secs(multiplier).min(RUNTIME_RECONNECT_MAX_DELAY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_reconnect_delay_backs_off_and_caps() {
        assert_eq!(runtime_reconnect_delay(0), Duration::from_secs(1));
        assert_eq!(runtime_reconnect_delay(1), Duration::from_secs(1));
        assert_eq!(runtime_reconnect_delay(2), Duration::from_secs(2));
        assert_eq!(runtime_reconnect_delay(3), Duration::from_secs(4));
        assert_eq!(runtime_reconnect_delay(4), RUNTIME_RECONNECT_MAX_DELAY);
        assert_eq!(runtime_reconnect_delay(20), RUNTIME_RECONNECT_MAX_DELAY);
    }
}
