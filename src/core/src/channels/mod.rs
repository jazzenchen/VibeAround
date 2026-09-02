//! ACP-native channel manager: hosts channel plugins and routes traffic.
//!
//! Web and stdio plugins both enter through `ChannelInput` and are routed
//! into workspace threads.
//!
//! Module layout:
//! - `types`            — wire types: `ChannelEnvelope`, `ChannelInput`, `ChannelOutput`
//! - `prompt`           — unified conversation ingress + workspace-thread commands
//! - `bridge_handler`   — `ChannelBridgeHandler` (notification + permission forwarding)
//! - `monitor`          — Dashboard-facing facade over `process::Supervisor`
//! - `plugin_runner`    — one stdio plugin generation + concrete factory
//! - `manifest`         — `ChannelPluginManifest`
//! - `plugin_host`      — runtime registry + pending permissions map
//! - `plugin_runtime`   — enum wrapper around Stdio / WebSocket runtimes
//! - `transport_stdio`  — ACP bridge to child plugin processes
//! - `transport_websocket` — in-process web chat channel

pub(crate) mod agent_protocol;
pub mod bridge_handler;
pub mod manifest;
pub mod monitor;
mod permission;
pub mod plugin_host;
pub mod plugin_paths;
pub mod plugin_runner;
pub mod plugin_runtime;
pub mod prompt;
pub mod subagent_handler;
pub mod transport_stdio;
pub mod transport_websocket;
pub mod types;

use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
use serde::Serialize;
use tokio::sync::mpsc;

use crate::plugins::DiscoveredPlugin;
use crate::workspace::WorkspaceThreadManager;

use self::manifest::ChannelPluginManifest;
use self::plugin_host::PluginHost;

pub use self::prompt::ConversationIngress;
pub use self::transport_websocket::WebChannelManager;
pub use self::types::{ChannelEnvelope, ChannelInput, ChannelOutput};

#[derive(Debug, Clone, Default, Serialize)]
pub struct ChannelSyncReport {
    pub registered: Vec<String>,
    pub restarted: Vec<String>,
    pub started: Vec<String>,
    pub stopped: Vec<String>,
    pub missing: Vec<String>,
}

/// Facade over the plugin host + monitor. Built once at daemon boot and
/// passed around as `Arc<ChannelManager>`.
pub struct ChannelManager {
    plugin_host: Arc<PluginHost>,
    /// Channel for fire-and-forget input dispatch.
    /// `handle_input` sends here; the processing loop runs on a dedicated
    /// task owned by the server startup path.
    input_tx: mpsc::UnboundedSender<ChannelInput>,
    workspace_thread_manager: Arc<WorkspaceThreadManager>,
    ingress: Arc<ConversationIngress>,
    monitor: Arc<monitor::ChannelMonitor>,
}

impl ChannelManager {
    pub fn new(
        workspace_thread_manager: Arc<WorkspaceThreadManager>,
    ) -> (Self, mpsc::UnboundedReceiver<ChannelInput>) {
        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let plugin_host = Arc::new(PluginHost::new(input_tx.clone()));
        let ingress = ConversationIngress::new(
            Arc::clone(&workspace_thread_manager),
            Arc::clone(&plugin_host),
        );
        let (change_tx, _) = tokio::sync::broadcast::channel::<()>(64);
        let monitor = monitor::ChannelMonitor::new(
            Arc::clone(&ingress),
            input_tx.clone(),
            Arc::clone(&plugin_host),
            change_tx,
        );
        plugin_host.set_monitor(Arc::downgrade(&monitor));
        (
            Self {
                plugin_host,
                input_tx,
                workspace_thread_manager,
                ingress,
                monitor,
            },
            input_rx,
        )
    }

    pub fn plugin_host(&self) -> Arc<PluginHost> {
        Arc::clone(&self.plugin_host)
    }

    pub fn monitor(&self) -> Arc<monitor::ChannelMonitor> {
        Arc::clone(&self.monitor)
    }

    /// Register a channel plugin with the supervisor. The monitor spawns it
    /// immediately (without waiting for the next 5s tick) and keeps it alive
    /// via its respawn + watchdog loop.
    ///
    /// Returns `true` if the manifest was built and registered, `false` if
    /// the channel lacks config (plugin disabled).
    pub async fn register_plugin(&self, channel_name: &str, plugin: &DiscoveredPlugin) -> bool {
        let manifest =
            match ChannelPluginManifest::from_discovered(channel_name.to_string(), plugin) {
                Some(manifest) => manifest,
                None => {
                    tracing::info!(
                        "[{}] config=missing channels.{} — plugin disabled",
                        channel_name,
                        channel_name
                    );
                    return false;
                }
            };
        self.monitor().register(manifest).await
    }

    pub async fn sync_configured_plugins(&self) -> ChannelSyncReport {
        let cfg = crate::config::reload();
        let discovered_plugins = crate::plugins::channel::discover();
        let desired = cfg.channel_names();
        let desired_instances = desired
            .iter()
            .cloned()
            .map(|kind| (kind.clone(), kind))
            .collect::<std::collections::BTreeMap<_, _>>();
        let desired_set = desired_instances
            .keys()
            .cloned()
            .collect::<std::collections::HashSet<_>>();
        let monitor = self.monitor();
        let registered = monitor.registered_instances().await;
        let registered_set = registered
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<_>>();
        let mut report = ChannelSyncReport::default();

        for instance_id in registered {
            if desired_set.contains(&instance_id) {
                continue;
            }
            if monitor.unregister(&instance_id).await.is_ok() {
                report.stopped.push(instance_id);
            }
        }

        for (instance_id, kind) in desired_instances {
            let Some(plugin) = discovered_plugins.get(&kind) else {
                if registered_set.contains(&instance_id)
                    && monitor.unregister(&instance_id).await.is_ok()
                {
                    report.stopped.push(instance_id.clone());
                }
                report.missing.push(instance_id);
                continue;
            };
            if !registered_set.contains(&instance_id) {
                if self.register_plugin(&kind, plugin).await {
                    report.registered.push(instance_id);
                }
                continue;
            }

            // Re-register to capture the latest plugin factory state.
            if monitor.unregister(&instance_id).await.is_ok()
                && self.register_plugin(&kind, plugin).await
            {
                report.restarted.push(instance_id);
            }
        }

        report.registered.sort();
        report.restarted.sort();
        report.started.sort();
        report.stopped.sort();
        report.missing.sort();
        report
    }

    pub fn start_internal_plugin(
        &self,
        channel_name: &str,
        outbound_tx: mpsc::UnboundedSender<ChannelOutput>,
    ) {
        self.plugin_host
            .register_websocket_plugin(channel_name.to_string(), outbound_tx);
        crate::proc_log!(
            info,
            kind = crate::process::ProcessKind::ChannelPlugin,
            label = channel_name,
            event = "registered_internal"
        );
    }

    /// Fire-and-forget: enqueue input for async processing. `Send`-safe
    /// because it only does a channel send.
    pub fn handle_input(&self, input: ChannelInput) {
        let _ = self.input_tx.send(input);
    }

    /// Route a single input without waiting for route work or plugin output.
    pub fn process_input(&self, input: ChannelInput) {
        self.ingress.dispatch(input);
    }

    pub fn ingress(&self) -> Arc<ConversationIngress> {
        Arc::clone(&self.ingress)
    }

    pub fn workspace_thread_manager(&self) -> Arc<WorkspaceThreadManager> {
        Arc::clone(&self.workspace_thread_manager)
    }

    pub fn send_output(&self, output: ChannelOutput) {
        self.plugin_host.send_output(output);
    }

    pub async fn respond_permission(
        &self,
        channel_instance_id: &str,
        request_id: &str,
        response: acp::RequestPermissionResponse,
    ) -> Result<(), String> {
        self.plugin_host
            .respond_permission(channel_instance_id, request_id, response)
            .await
    }

    pub async fn shutdown_all(&self) {
        // Cancel every supervised plugin bridge first so they wind down
        // cleanly, then drop the host-side routing + pending permissions.
        self.monitor.shutdown_all().await;
        self.plugin_host.shutdown_all().await;
    }
}
