//! One stdio channel-plugin process generation and its concrete factory.
//!
//! [`ChannelPluginRunner`] owns the protocol-side resources for exactly one
//! supervised spawn. [`ChannelPluginRunnerFactory`] creates a fresh output
//! channel and runtime for every generation, then publishes that runtime to
//! [`PluginHost`] before handing the runner to the process supervisor.

use std::sync::Arc;

use tokio::sync::mpsc;

use super::manifest::ChannelPluginManifest;
use super::plugin_host::PluginHost;
use super::transport_stdio::{run_acp_plugin_bridge, StdioBridgeMessage, StdioPluginRuntime};
use super::{ChannelInput, ConversationIngress};
use crate::process::bridge::{
    BridgeFactory, BridgeFuture, CancelSignal, ProcessBridge, StdioPipes,
};

/// The protocol-side owner for one stdio channel-plugin spawn.
pub struct ChannelPluginRunner {
    pub manifest: ChannelPluginManifest,
    pub input_tx: mpsc::UnboundedSender<ChannelInput>,
    pub(crate) output_rx: mpsc::UnboundedReceiver<StdioBridgeMessage>,
    pub ingress: Arc<ConversationIngress>,
    pub plugin_host: Arc<PluginHost>,
    pub runtime: Arc<StdioPluginRuntime>,
}

impl ProcessBridge for ChannelPluginRunner {
    fn run(self: Box<Self>, pipes: StdioPipes, cancel: CancelSignal) -> BridgeFuture {
        let this = *self;
        Box::pin(
            async move { run_acp_plugin_bridge(this, pipes.stdin, pipes.stdout, cancel).await },
        )
    }
}

/// Builds a fresh [`ChannelPluginRunner`] for every supervised spawn.
pub struct ChannelPluginRunnerFactory {
    manifest: ChannelPluginManifest,
    input_tx: mpsc::UnboundedSender<ChannelInput>,
    ingress: Arc<ConversationIngress>,
    plugin_host: Arc<PluginHost>,
}

impl ChannelPluginRunnerFactory {
    pub fn new(
        manifest: ChannelPluginManifest,
        input_tx: mpsc::UnboundedSender<ChannelInput>,
        ingress: Arc<ConversationIngress>,
        plugin_host: Arc<PluginHost>,
    ) -> Self {
        Self {
            manifest,
            input_tx,
            ingress,
            plugin_host,
        }
    }

    pub fn into_bridge_factory(self) -> BridgeFactory {
        Box::new(move || Box::new(self.create()) as Box<dyn ProcessBridge>)
    }

    fn create(&self) -> ChannelPluginRunner {
        // One plugin generation owns one ACP connection, so outputs from its
        // routes and their completion barriers share this transport FIFO.
        // It preserves wire order but provides no backpressure; the previous
        // try_send limit could drop output before a later successful barrier.
        let (output_tx, output_rx) = mpsc::unbounded_channel();
        let runtime = Arc::new(StdioPluginRuntime::new(
            self.manifest.instance_id.clone(),
            output_tx,
        ));
        self.plugin_host
            .replace_stdio_runtime(&self.manifest.instance_id, Arc::clone(&runtime));

        ChannelPluginRunner {
            manifest: self.manifest.clone(),
            input_tx: self.input_tx.clone(),
            output_rx,
            ingress: Arc::clone(&self.ingress),
            plugin_host: Arc::clone(&self.plugin_host),
            runtime,
        }
    }
}
