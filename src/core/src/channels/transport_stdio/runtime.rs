//! `StdioPluginRuntime` — the routing-side half of an stdio plugin.
//!
//! After the PR2 migration, this type is intentionally small. It only
//! carries the output sender that `PluginHost::send_output` writes to;
//! the supervisor owns the `Child` and the bridge thread owns the
//! receiver. Each time the plugin is (re)spawned, a fresh runtime is
//! built with a fresh `(output_tx, output_rx)` pair and registered via
//! `PluginHost::replace_stdio_runtime` so routing always points at the
//! live bridge.

use tokio::sync::mpsc;

use super::super::ChannelOutput;

#[derive(Debug)]
pub struct StdioPluginRuntime {
    instance_id: String,
    output_tx: mpsc::Sender<ChannelOutput>,
}

impl StdioPluginRuntime {
    pub fn new(instance_id: impl Into<String>, output_tx: mpsc::Sender<ChannelOutput>) -> Self {
        Self {
            instance_id: instance_id.into(),
            output_tx,
        }
    }

    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    pub fn send_output_now(&self, output: ChannelOutput) -> Result<(), String> {
        self.output_tx.try_send(output).map_err(|error| {
            format!("failed to enqueue realtime output for ACP plugin bridge: {error}")
        })
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
        assert_eq!(rx.try_recv().unwrap(), output("first"));
    }
}
