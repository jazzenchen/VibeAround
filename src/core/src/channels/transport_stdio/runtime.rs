//! `StdioPluginRuntime` — the routing-side half of an stdio plugin.
//!
//! After the PR2 migration, this type is intentionally small. It only
//! carries the output sender that `PluginHost::send_output` writes to;
//! the supervisor owns the `Child` and the bridge thread owns the
//! receiver. Each time the plugin is (re)spawned, a fresh runtime is
//! built with a fresh `(output_tx, output_rx)` pair and registered via
//! `PluginHost::replace_stdio_runtime` so routing always points at the
//! live bridge.

use tokio::sync::{mpsc, oneshot};

use super::super::ChannelOutput;

#[derive(Debug)]
pub(crate) enum StdioBridgeMessage {
    Output(ChannelOutput),
    Barrier(oneshot::Sender<Result<(), String>>),
}

#[derive(Debug)]
pub struct StdioPluginRuntime {
    instance_id: String,
    output_tx: mpsc::Sender<StdioBridgeMessage>,
}

impl StdioPluginRuntime {
    pub(crate) fn new(
        instance_id: impl Into<String>,
        output_tx: mpsc::Sender<StdioBridgeMessage>,
    ) -> Self {
        Self {
            instance_id: instance_id.into(),
            output_tx,
        }
    }

    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    pub fn send_output_now(&self, output: ChannelOutput) -> Result<(), String> {
        self.output_tx
            .try_send(StdioBridgeMessage::Output(output))
            .map_err(|error| {
                format!("failed to enqueue realtime output for ACP plugin bridge: {error}")
            })
    }

    pub(crate) fn enqueue_barrier(&self, reply: oneshot::Sender<Result<(), String>>) {
        match self.output_tx.try_send(StdioBridgeMessage::Barrier(reply)) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(message)) => {
                let output_tx = self.output_tx.clone();
                tokio::spawn(async move {
                    if let Err(error) = output_tx.send(message).await {
                        let StdioBridgeMessage::Barrier(reply) = error.0 else {
                            unreachable!("enqueue_barrier only sends barriers");
                        };
                        let _ =
                            reply
                                .send(Err("ACP plugin output forwarder closed before the barrier"
                                    .to_string()));
                    }
                });
            }
            Err(mpsc::error::TrySendError::Closed(message)) => {
                let StdioBridgeMessage::Barrier(reply) = message else {
                    unreachable!("enqueue_barrier only sends barriers");
                };
                let _ = reply.send(Err("ACP plugin output forwarder is closed".to_string()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::RouteKey;

    fn output(text: &str) -> ChannelOutput {
        ChannelOutput::SystemText {
            route: RouteKey::new("slack", "chat-1"),
            text: text.to_string(),
            reply_to: None,
        }
    }

    #[test]
    fn full_transport_buffer_rejects_without_waiting() {
        let (tx, mut rx) = mpsc::channel(1);
        let runtime = StdioPluginRuntime::new("slack-work", tx);

        runtime.send_output_now(output("first")).unwrap();
        let error = runtime.send_output_now(output("second")).unwrap_err();

        assert!(error.contains("no available capacity"));
        let StdioBridgeMessage::Output(first) = rx.try_recv().unwrap() else {
            panic!("expected output");
        };
        assert_eq!(first, output("first"));
    }

    #[tokio::test]
    async fn barrier_follows_prior_output_and_acks_when_forwarded() {
        let (tx, mut rx) = mpsc::channel(2);
        let runtime = StdioPluginRuntime::new("slack-work", tx);
        runtime.send_output_now(output("first")).unwrap();
        let (reply, done) = oneshot::channel();
        runtime.enqueue_barrier(reply);

        assert!(matches!(
            rx.recv().await,
            Some(StdioBridgeMessage::Output(_))
        ));
        let Some(StdioBridgeMessage::Barrier(reply)) = rx.recv().await else {
            panic!("expected barrier after output");
        };
        reply.send(Ok(())).unwrap();
        assert_eq!(done.await.unwrap(), Ok(()));
    }

    #[tokio::test]
    async fn full_buffer_waits_to_enqueue_barrier() {
        let (tx, mut rx) = mpsc::channel(1);
        let runtime = StdioPluginRuntime::new("slack-work", tx);
        runtime.send_output_now(output("first")).unwrap();
        let (reply, mut done) = oneshot::channel();
        runtime.enqueue_barrier(reply);

        assert!(done.try_recv().is_err());
        assert!(matches!(
            rx.recv().await,
            Some(StdioBridgeMessage::Output(_))
        ));
        let Some(StdioBridgeMessage::Barrier(reply)) = rx.recv().await else {
            panic!("expected queued barrier");
        };
        reply.send(Ok(())).unwrap();
        assert_eq!(done.await.unwrap(), Ok(()));
    }

    #[tokio::test]
    async fn closed_buffer_fails_barrier() {
        let (tx, rx) = mpsc::channel(1);
        let runtime = StdioPluginRuntime::new("slack-work", tx);
        drop(rx);
        let (reply, done) = oneshot::channel();
        runtime.enqueue_barrier(reply);

        assert!(done.await.unwrap().unwrap_err().contains("closed"));
    }
}
