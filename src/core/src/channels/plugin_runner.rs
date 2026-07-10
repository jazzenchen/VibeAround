//! One stdio channel-plugin process generation and its concrete factory.
//!
//! [`ChannelPluginRunner`] owns the protocol-side resources for exactly one
//! supervised spawn. [`ChannelPluginRunnerFactory`] creates a fresh output
//! channel and runtime for every generation, then publishes that runtime to
//! [`PluginHost`] before handing the runner to the process supervisor.

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::process::bridge::{
    BridgeFactory, BridgeFuture, CancelSignal, ProcessBridge, StdioPipes,
};
use crate::workspace::WorkspaceThreadManager;

use super::plugin_host::PluginHost;
use super::transport_stdio::{run_acp_plugin_bridge, QueuedChannelOutput, StdioPluginRuntime};
use super::ChannelInput;

/// The protocol-side owner for one stdio channel-plugin spawn.
pub struct ChannelPluginRunner {
    pub channel_kind: String,
    pub raw_config: serde_json::Value,
    pub input_tx: mpsc::UnboundedSender<ChannelInput>,
    pub output_rx: mpsc::UnboundedReceiver<QueuedChannelOutput>,
    pub workspace_thread_manager: Arc<WorkspaceThreadManager>,
    pub plugin_host: Arc<PluginHost>,
    pub runtime: Arc<StdioPluginRuntime>,
}

impl ProcessBridge for ChannelPluginRunner {
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

/// Builds a fresh [`ChannelPluginRunner`] for every supervised spawn.
pub struct ChannelPluginRunnerFactory {
    channel_kind: String,
    input_tx: mpsc::UnboundedSender<ChannelInput>,
    workspace_thread_manager: Arc<WorkspaceThreadManager>,
    plugin_host: Arc<PluginHost>,
}

impl ChannelPluginRunnerFactory {
    pub fn new(
        channel_kind: impl Into<String>,
        input_tx: mpsc::UnboundedSender<ChannelInput>,
        workspace_thread_manager: Arc<WorkspaceThreadManager>,
        plugin_host: Arc<PluginHost>,
    ) -> Self {
        Self {
            channel_kind: channel_kind.into(),
            input_tx,
            workspace_thread_manager,
            plugin_host,
        }
    }

    pub fn into_bridge_factory(self) -> BridgeFactory {
        Box::new(move || Box::new(self.create()) as Box<dyn ProcessBridge>)
    }

    fn create(&self) -> ChannelPluginRunner {
        let (output_tx, output_rx) = mpsc::unbounded_channel();
        let runtime = Arc::new(StdioPluginRuntime::new(
            self.channel_kind.clone(),
            output_tx,
        ));
        self.plugin_host
            .replace_stdio_runtime(&self.channel_kind, Arc::clone(&runtime));
        let raw_config = crate::config::ensure_loaded()
            .channel_raw_config(&self.channel_kind)
            .unwrap_or_else(|| serde_json::json!({}));

        ChannelPluginRunner {
            channel_kind: self.channel_kind.clone(),
            raw_config,
            input_tx: self.input_tx.clone(),
            output_rx,
            workspace_thread_manager: Arc::clone(&self.workspace_thread_manager),
            plugin_host: Arc::clone(&self.plugin_host),
            runtime,
        }
    }
}
