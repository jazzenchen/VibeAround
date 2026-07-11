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

    pub async fn send_output(&self, output: ChannelOutput) -> Result<(), String> {
        self.output_tx.send(output).await.map_err(|error| {
            let message = format!("failed to send output to ACP plugin bridge: {error}");
            tracing::info!("[{}] {}", self.instance_id, message);
            message
        })
    }
}
