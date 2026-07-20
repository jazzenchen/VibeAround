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
    output_tx: mpsc::UnboundedSender<StdioBridgeMessage>,
}

impl StdioPluginRuntime {
    pub(crate) fn new(
        instance_id: impl Into<String>,
        output_tx: mpsc::UnboundedSender<StdioBridgeMessage>,
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
            .send(StdioBridgeMessage::Output(output))
            .map_err(|error| {
                format!("failed to enqueue realtime output for ACP plugin bridge: {error}")
            })
    }

    pub(crate) fn enqueue_barrier(&self, reply: oneshot::Sender<Result<(), String>>) {
        if let Err(error) = self.output_tx.send(StdioBridgeMessage::Barrier(reply)) {
            let StdioBridgeMessage::Barrier(reply) = error.0 else {
                unreachable!("enqueue_barrier only sends barriers");
            };
            let _ = reply.send(Err("ACP plugin output forwarder is closed".to_string()));
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

    #[tokio::test]
    async fn queued_outputs_are_not_dropped_before_the_barrier() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let runtime = StdioPluginRuntime::new("slack-work", tx);

        for index in 0..1024 {
            runtime
                .send_output_now(output(&format!("message-{index}")))
                .unwrap();
        }
        let (reply, done) = oneshot::channel();
        runtime.enqueue_barrier(reply);

        for index in 0..1024 {
            let Some(StdioBridgeMessage::Output(next)) = rx.recv().await else {
                panic!("expected output before barrier");
            };
            assert_eq!(next, output(&format!("message-{index}")));
        }
        let Some(StdioBridgeMessage::Barrier(reply)) = rx.recv().await else {
            panic!("expected barrier after queued output");
        };
        reply.send(Ok(())).unwrap();
        assert_eq!(done.await.unwrap(), Ok(()));
    }

    #[tokio::test]
    async fn closed_forwarder_rejects_output_and_barrier() {
        let (tx, rx) = mpsc::unbounded_channel();
        let runtime = StdioPluginRuntime::new("slack-work", tx);
        drop(rx);
        assert!(runtime.send_output_now(output("lost")).is_err());
        let (reply, done) = oneshot::channel();
        runtime.enqueue_barrier(reply);

        assert!(done.await.unwrap().unwrap_err().contains("closed"));
    }
}
