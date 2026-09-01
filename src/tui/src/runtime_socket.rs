use std::sync::Arc;

use futures_util::StreamExt;
use serde_json::Value;
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use va_client::events::{
    agents_runtime_ws, channels_ws, decode_agents_runtime_event, decode_channels_event,
    decode_tunnels_event, tunnels_ws, WebSocketSpec,
};
use va_client::runtime::{AgentRuntime, ChannelRuntime, TunnelRuntime};

use crate::socket_retry::{
    socket_retry_after_failure, SocketRetry, SOCKET_RETRY_INTERVAL, SOCKET_RETRY_LIMIT,
};
use crate::transport::SharedEndpoint;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeStream {
    Channels,
    Tunnels,
    Agents,
}

#[derive(Debug)]
pub(crate) enum RuntimeSocketEvent {
    Channels(Vec<ChannelRuntime>),
    Tunnels(Vec<TunnelRuntime>),
    Agents(Vec<AgentRuntime>),
    Error {
        stream: RuntimeStream,
        message: String,
    },
}

pub(crate) async fn run_runtime_sockets(
    endpoint: Arc<SharedEndpoint>,
    incoming: mpsc::UnboundedSender<RuntimeSocketEvent>,
    reconnect: watch::Receiver<()>,
) {
    let tasks = [
        tokio::spawn(run_channels_socket(
            endpoint.clone(),
            incoming.clone(),
            reconnect.clone(),
        )),
        tokio::spawn(run_tunnels_socket(
            endpoint.clone(),
            incoming.clone(),
            reconnect.clone(),
        )),
        tokio::spawn(run_agents_socket(endpoint, incoming, reconnect)),
    ];
    for task in tasks {
        let _ = task.await;
    }
}

async fn run_channels_socket(
    endpoint: Arc<SharedEndpoint>,
    incoming: mpsc::UnboundedSender<RuntimeSocketEvent>,
    reconnect: watch::Receiver<()>,
) {
    run_snapshot_socket(
        endpoint,
        RuntimeStream::Channels,
        channels_ws(),
        incoming,
        |value| decode_channels_event(value).map(RuntimeSocketEvent::Channels),
        reconnect,
    )
    .await;
}

async fn run_tunnels_socket(
    endpoint: Arc<SharedEndpoint>,
    incoming: mpsc::UnboundedSender<RuntimeSocketEvent>,
    reconnect: watch::Receiver<()>,
) {
    run_snapshot_socket(
        endpoint,
        RuntimeStream::Tunnels,
        tunnels_ws(),
        incoming,
        |value| decode_tunnels_event(value).map(RuntimeSocketEvent::Tunnels),
        reconnect,
    )
    .await;
}

async fn run_agents_socket(
    endpoint: Arc<SharedEndpoint>,
    incoming: mpsc::UnboundedSender<RuntimeSocketEvent>,
    reconnect: watch::Receiver<()>,
) {
    run_snapshot_socket(
        endpoint,
        RuntimeStream::Agents,
        agents_runtime_ws(),
        incoming,
        |value| decode_agents_runtime_event(value).map(RuntimeSocketEvent::Agents),
        reconnect,
    )
    .await;
}

async fn run_snapshot_socket(
    endpoint: Arc<SharedEndpoint>,
    stream: RuntimeStream,
    socket: WebSocketSpec,
    incoming: mpsc::UnboundedSender<RuntimeSocketEvent>,
    decode: fn(Value) -> va_client::Result<RuntimeSocketEvent>,
    mut reconnect: watch::Receiver<()>,
) {
    let label = socket.path.clone();
    let mut failed_attempts = 0;

    loop {
        // A daemon restart rotates the token, so re-read the auth file and
        // recompute the URL before every attempt.
        endpoint.refresh_token();
        let url = endpoint.websocket_url(&socket);
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
                match socket_retry_after_failure(failed_attempts) {
                    SocketRetry::RetryAfter(delay) => tokio::time::sleep(delay).await,
                    SocketRetry::GiveUp => {
                        // Only wake for a reconnect requested after this
                        // stream reported giving up.
                        reconnect.mark_unchanged();
                        if incoming
                            .send(RuntimeSocketEvent::Error {
                                stream,
                                message: format!(
                                    "{label} unreachable; gave up after {SOCKET_RETRY_LIMIT} attempts"
                                ),
                            })
                            .is_err()
                        {
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

        // The connection itself succeeded, so a drop starts a fresh retry
        // cycle rather than counting toward the failure limit.
        tokio::time::sleep(SOCKET_RETRY_INTERVAL).await;
    }
}
