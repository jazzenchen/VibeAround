//! One stdio channel-plugin process generation and its concrete factory.
//!
//! [`ChannelPluginRunner`] owns the protocol-side resources for exactly one
//! supervised spawn. [`ChannelPluginRunnerFactory`] creates a fresh output
//! channel and runtime for every generation, then publishes that runtime to
//! [`PluginHost`] before handing the runner to the process supervisor.

use std::sync::Arc;

use tokio::sync::mpsc;

use super::plugin_host::PluginHost;
use super::transport_stdio::{run_acp_plugin_bridge, StdioPluginRuntime};
use super::{ChannelInput, ConversationIngress};
use crate::process::bridge::{
    BridgeFactory, BridgeFuture, CancelSignal, ProcessBridge, StdioPipes,
};

/// Small transport buffer only; it is not replayed across bridge/process loss.
const CHANNEL_OUTPUT_BUFFER: usize = 256;

/// The protocol-side owner for one stdio channel-plugin spawn.
pub struct ChannelPluginRunner {
    pub channel_kind: String,
    pub instance_id: String,
    pub actor_id: String,
    pub raw_config: serde_json::Value,
    pub input_tx: mpsc::UnboundedSender<ChannelInput>,
    pub output_rx: mpsc::Receiver<super::ChannelOutput>,
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
    channel_kind: String,
    instance_id: String,
    actor_id: String,
    raw_config: serde_json::Value,
    input_tx: mpsc::UnboundedSender<ChannelInput>,
    ingress: Arc<ConversationIngress>,
    plugin_host: Arc<PluginHost>,
}

impl ChannelPluginRunnerFactory {
    pub fn new(
        channel_kind: impl Into<String>,
        instance_id: impl Into<String>,
        actor_id: impl Into<String>,
        raw_config: serde_json::Value,
        input_tx: mpsc::UnboundedSender<ChannelInput>,
        ingress: Arc<ConversationIngress>,
        plugin_host: Arc<PluginHost>,
    ) -> Self {
        Self {
            channel_kind: channel_kind.into(),
            instance_id: instance_id.into(),
            actor_id: actor_id.into(),
            raw_config,
            input_tx,
            ingress,
            plugin_host,
        }
    }

    pub fn into_bridge_factory(self) -> BridgeFactory {
        Box::new(move || Box::new(self.create()) as Box<dyn ProcessBridge>)
    }

    fn create(&self) -> ChannelPluginRunner {
        let (output_tx, output_rx) = mpsc::channel(CHANNEL_OUTPUT_BUFFER);
        let runtime = Arc::new(StdioPluginRuntime::new(self.instance_id.clone(), output_tx));
        self.plugin_host
            .replace_stdio_runtime(&self.instance_id, Arc::clone(&runtime));

        ChannelPluginRunner {
            channel_kind: self.channel_kind.clone(),
            instance_id: self.instance_id.clone(),
            actor_id: self.actor_id.clone(),
            raw_config: self.raw_config.clone(),
            input_tx: self.input_tx.clone(),
            output_rx,
            ingress: Arc::clone(&self.ingress),
            plugin_host: Arc::clone(&self.plugin_host),
            runtime,
        }
    }
}
