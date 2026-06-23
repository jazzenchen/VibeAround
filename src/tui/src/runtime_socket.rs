use futures_util::StreamExt;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use va_client::endpoint::ServerEndpoint;
use va_client::events::{
    agents_runtime_ws, channels_ws, decode_agents_runtime_event, decode_channels_event,
    decode_tunnels_event, tunnels_ws, WebSocketSpec,
};
use va_client::runtime::{AgentRuntime, ChannelRuntime, TunnelRuntime};

#[derive(Debug)]
pub(crate) enum RuntimeSocketEvent {
    Channels(Vec<ChannelRuntime>),
    Tunnels(Vec<TunnelRuntime>),
    Agents(Vec<AgentRuntime>),
    Error(String),
}

pub(crate) async fn run_runtime_sockets(
    endpoint: ServerEndpoint,
    incoming: mpsc::UnboundedSender<RuntimeSocketEvent>,
) {
    let tasks = [
        tokio::spawn(run_channels_socket(endpoint.clone(), incoming.clone())),
        tokio::spawn(run_tunnels_socket(endpoint.clone(), incoming.clone())),
        tokio::spawn(run_agents_socket(endpoint, incoming)),
    ];
    for task in tasks {
        let _ = task.await;
    }
}

async fn run_channels_socket(
    endpoint: ServerEndpoint,
    incoming: mpsc::UnboundedSender<RuntimeSocketEvent>,
) {
    run_snapshot_socket(endpoint, channels_ws(), incoming, |value| {
        decode_channels_event(value).map(RuntimeSocketEvent::Channels)
    })
    .await;
}

async fn run_tunnels_socket(
    endpoint: ServerEndpoint,
    incoming: mpsc::UnboundedSender<RuntimeSocketEvent>,
) {
    run_snapshot_socket(endpoint, tunnels_ws(), incoming, |value| {
        decode_tunnels_event(value).map(RuntimeSocketEvent::Tunnels)
    })
    .await;
}

async fn run_agents_socket(
    endpoint: ServerEndpoint,
    incoming: mpsc::UnboundedSender<RuntimeSocketEvent>,
) {
    run_snapshot_socket(endpoint, agents_runtime_ws(), incoming, |value| {
        decode_agents_runtime_event(value).map(RuntimeSocketEvent::Agents)
    })
    .await;
}

async fn run_snapshot_socket(
    endpoint: ServerEndpoint,
    socket: WebSocketSpec,
    incoming: mpsc::UnboundedSender<RuntimeSocketEvent>,
    decode: fn(Value) -> va_client::Result<RuntimeSocketEvent>,
) {
    let label = socket.path.clone();
    let url = endpoint.websocket_url(&socket);
    let (mut ws, _) = match connect_async(&url).await {
        Ok(connection) => connection,
        Err(error) => {
            let _ = incoming.send(RuntimeSocketEvent::Error(format!(
                "failed to connect {label}: {error}"
            )));
            return;
        }
    };

    while let Some(frame) = ws.next().await {
        match frame {
            Ok(Message::Text(text)) => match serde_json::from_str::<Value>(&text) {
                Ok(value) => match decode(value) {
                    Ok(event) => {
                        let _ = incoming.send(event);
                    }
                    Err(error) => {
                        let _ = incoming.send(RuntimeSocketEvent::Error(format!(
                            "failed to decode {label}: {error}"
                        )));
                    }
                },
                Err(error) => {
                    let _ = incoming.send(RuntimeSocketEvent::Error(format!(
                        "failed to parse {label}: {error}"
                    )));
                }
            },
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(error) => {
                let _ = incoming.send(RuntimeSocketEvent::Error(format!(
                    "{label} websocket read failed: {error}"
                )));
                break;
            }
        }
    }
}
