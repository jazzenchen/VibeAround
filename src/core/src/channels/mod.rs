//! ACP-native channel manager: hosts channel plugins and routes traffic.
//!
//! Web and stdio plugins both enter through `ChannelInput` and are routed
//! into workspace threads.
//!
//! Module layout:
//! - `types`            — wire types: `ChannelEnvelope`, `ChannelInput`, `ChannelOutput`
//! - `prompt`           — `handle_channel_input` + workspace-thread commands
//! - `bridge_handler`   — `ChannelBridgeHandler` (notification + permission forwarding)
//! - `monitor`          — Dashboard-facing facade over `process::Supervisor`
//! - `plugin_bridge`    — `ChannelPluginBridge` (`ProcessBridge` impl for stdio plugins)
//! - `manifest`         — `ChannelPluginManifest`
//! - `plugin_host`      — runtime registry + pending permissions map
//! - `plugin_runtime`   — enum wrapper around Stdio / WebSocket runtimes
//! - `transport_stdio`  — ACP bridge to child plugin processes
//! - `transport_websocket` — in-process web chat channel

pub(crate) mod agent_protocol;
pub mod bridge_handler;
pub mod manifest;
pub mod monitor;
pub mod outbox;
pub mod plugin_bridge;
pub mod plugin_host;
pub mod plugin_runtime;
pub mod prompt;
pub mod subagent_handler;
pub mod transport_stdio;
pub mod transport_websocket;
pub mod types;

use std::sync::{Arc, Mutex as StdMutex};

use agent_client_protocol::schema as acp;
use serde::Serialize;
use tokio::sync::mpsc;

use crate::plugins::DiscoveredPlugin;
use crate::workspace::WorkspaceThreadManager;

use self::manifest::ChannelPluginManifest;
use self::plugin_host::PluginHost;

// Re-exports so the rest of the crate keeps its existing import paths.
pub use self::prompt::handle_channel_input;
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
    input_rx: StdMutex<Option<mpsc::UnboundedReceiver<ChannelInput>>>,
    workspace_thread_manager: Arc<WorkspaceThreadManager>,
    /// Lazy-initialised on first `register_plugin` call. The monitor is
    /// a thin facade over `process::Supervisor` — it owns the supervisor
    /// and its tick loop internally.
    monitor: StdMutex<Option<Arc<monitor::ChannelMonitor>>>,
}

impl ChannelManager {
    pub fn new(workspace_thread_manager: Arc<WorkspaceThreadManager>) -> Self {
        let (input_tx, input_rx) = mpsc::unbounded_channel();
        Self {
            plugin_host: Arc::new(PluginHost::new(input_tx.clone())),
            input_tx,
            input_rx: StdMutex::new(Some(input_rx)),
            workspace_thread_manager,
            monitor: StdMutex::new(None),
        }
    }

    pub fn plugin_host(&self) -> Arc<PluginHost> {
        Arc::clone(&self.plugin_host)
    }

    /// Return the monitor, initialising it on first call. Construction
    /// also spawns the underlying `process::Supervisor` tick loop.
    pub fn monitor(&self) -> Arc<monitor::ChannelMonitor> {
        let mut slot = self.monitor.lock().unwrap();
        if let Some(existing) = slot.as_ref() {
            return Arc::clone(existing);
        }
        let (change_tx, _) = tokio::sync::broadcast::channel::<()>(64);
        let m = monitor::ChannelMonitor::new(
            Arc::clone(&self.workspace_thread_manager),
            self.input_tx.clone(),
            Arc::clone(&self.plugin_host),
            change_tx,
        );
        // Weak back-pointer so the plugin bridge's `_va/heartbeat`
        // handler can call `touch(kind)` on the monitor.
        self.plugin_host.set_monitor(Arc::downgrade(&m));

        *slot = Some(Arc::clone(&m));
        m
    }

    /// Take the input receiver so the caller can drive the processing loop.
    /// Must be called exactly once (typically during daemon startup).
    pub fn take_input_rx(&self) -> Option<mpsc::UnboundedReceiver<ChannelInput>> {
        self.input_rx.lock().unwrap().take()
    }

    /// Register a channel plugin with the supervisor. The monitor spawns it
    /// immediately (without waiting for the next 5s tick) and keeps it alive
    /// via its respawn + watchdog loop.
    ///
    /// Returns `true` if the manifest was built and registered, `false` if
    /// the channel lacks config (plugin disabled).
    pub fn register_plugin(&self, channel_name: &str, plugin: &DiscoveredPlugin) -> bool {
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
        self.monitor().register(manifest);
        true
    }

    pub async fn sync_configured_plugins(&self) -> ChannelSyncReport {
        let cfg = crate::config::reload();
        let discovered_plugins = crate::plugins::channel::discover();
        let desired = cfg.channel_names();
        let desired_set = desired
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<_>>();
        let monitor = self.monitor();
        let registered = monitor.registered_kinds();
        let registered_set = registered
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<_>>();
        let mut report = ChannelSyncReport::default();

        for name in registered {
            if desired_set.contains(&name) {
                continue;
            }
            if monitor.force_stop(&name).await.is_ok() {
                report.stopped.push(name);
            }
        }

        for name in desired {
            let Some(plugin) = discovered_plugins.get(&name) else {
                if registered_set.contains(&name) && monitor.force_stop(&name).await.is_ok() {
                    report.stopped.push(name.clone());
                }
                report.missing.push(name);
                continue;
            };
            if !registered_set.contains(&name) {
                if self.register_plugin(&name, plugin) {
                    report.registered.push(name);
                }
                continue;
            }

            match monitor.status(&name) {
                Some(monitor::ChannelRunStatus::Stopped)
                | Some(monitor::ChannelRunStatus::Crashed)
                | Some(monitor::ChannelRunStatus::NotStarted) => {
                    if monitor.force_start(&name).is_ok() {
                        report.started.push(name);
                    }
                }
                _ => {
                    if monitor.force_restart(&name).await.is_ok() {
                        report.restarted.push(name);
                    }
                }
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
            kind = crate::process::registry::ProcessKind::ChannelPlugin,
            label = channel_name,
            event = "registered_internal"
        );
    }

    /// Fire-and-forget: enqueue input for async processing. `Send`-safe
    /// because it only does a channel send.
    pub fn handle_input(&self, input: ChannelInput) {
        let _ = self.input_tx.send(input);
    }

    /// Process a single input on the current executor.
    pub async fn process_input(&self, input: ChannelInput) {
        prompt::handle_channel_input(&self.workspace_thread_manager, &self.plugin_host, input)
            .await;
    }

    pub fn workspace_thread_manager(&self) -> Arc<WorkspaceThreadManager> {
        Arc::clone(&self.workspace_thread_manager)
    }

    pub async fn send_output(&self, output: ChannelOutput) {
        self.plugin_host.send_output(output).await;
    }

    pub fn respond_permission(
        &self,
        channel_kind: &str,
        request_id: &str,
        response: acp::RequestPermissionResponse,
    ) -> Result<(), String> {
        self.plugin_host
            .respond_permission(channel_kind, request_id, response)
    }

    pub async fn shutdown_all(&self) {
        // Cancel every supervised plugin bridge first so they wind down
        // cleanly, then drop the host-side routing + pending permissions.
        let monitor = self.monitor.lock().unwrap().clone();
        if let Some(monitor) = monitor {
            monitor.shutdown_all().await;
        }
        self.plugin_host.shutdown_all().await;
    }
}
