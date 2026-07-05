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
pub struct QueuedChannelOutput {
    pub output_id: Option<String>,
    pub output: ChannelOutput,
}

#[derive(Debug)]
pub struct StdioPluginRuntime {
    channel_kind: String,
    output_tx: mpsc::UnboundedSender<QueuedChannelOutput>,
}

impl StdioPluginRuntime {
    pub fn new(
        channel_kind: impl Into<String>,
        output_tx: mpsc::UnboundedSender<QueuedChannelOutput>,
    ) -> Self {
        Self {
            channel_kind: channel_kind.into(),
            output_tx,
        }
    }

    pub fn channel_kind(&self) -> &str {
        &self.channel_kind
    }

    pub fn send_output_now(&self, output: ChannelOutput) -> Result<(), String> {
        self.send_queued_output_now(None, output)
    }

    pub fn send_queued_output_now(
        &self,
        output_id: Option<String>,
        output: ChannelOutput,
    ) -> Result<(), String> {
        self.output_tx
            .send(QueuedChannelOutput { output_id, output })
            .map_err(|error| {
                let message = format!("failed to send output to ACP plugin bridge: {error}");
                tracing::info!("[{}] {}", self.channel_kind, message);
                message
            })
    }

    pub async fn send_output(&self, output: ChannelOutput) -> Result<(), String> {
        self.send_output_now(output)
    }

    /// No-op — lifecycle (cancel + reap) is the supervisor's job now.
    /// Kept so call sites that iterate `PluginRuntime` variants compile;
    /// will be removed once `PluginRuntime` is cleaned up.
    pub async fn shutdown(&self) {}
}
