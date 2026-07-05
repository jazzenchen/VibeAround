//! `ChannelPluginBridge` — the manager-owned `ProcessBridge` for stdio
//! channel plugins.
//!
//! On each (re)spawn, the [`process::Supervisor`] hands the bridge a fresh
//! [`StdioPipes`]. The bridge runs [`run_acp_plugin_bridge`] to completion
//! or until the supervisor cancels it.
//!
//! [`process::Supervisor`]: crate::process::Supervisor
//! [`StdioPipes`]: crate::process::StdioPipes

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::process::bridge::{BridgeFuture, CancelSignal, ProcessBridge, StdioPipes};
use crate::workspace::WorkspaceThreadManager;

use super::plugin_host::PluginHost;
use super::transport_stdio::{run_acp_plugin_bridge, QueuedChannelOutput, StdioPluginRuntime};
use super::ChannelInput;

/// The per-spawn ACP bridge for a stdio channel plugin.
pub struct ChannelPluginBridge {
    pub channel_kind: String,
    pub raw_config: serde_json::Value,
    pub input_tx: mpsc::UnboundedSender<ChannelInput>,
    pub output_rx: mpsc::UnboundedReceiver<QueuedChannelOutput>,
    pub workspace_thread_manager: Arc<WorkspaceThreadManager>,
    pub plugin_host: Arc<PluginHost>,
    pub runtime: Arc<StdioPluginRuntime>,
}

impl ProcessBridge for ChannelPluginBridge {
    fn run(self: Box<Self>, pipes: StdioPipes, cancel: CancelSignal) -> BridgeFuture {
        let this = *self;
        Box::pin(async move {
            run_acp_plugin_bridge(
                this.channel_kind,
                this.raw_config,
                pipes.stdin,
                pipes.stdout,
                this.input_tx,
                this.output_rx,
                this.workspace_thread_manager,
                this.plugin_host,
                this.runtime,
                cancel,
            )
            .await
        })
    }
}
