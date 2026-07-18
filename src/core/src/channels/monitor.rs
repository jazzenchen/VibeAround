//! `ChannelMonitor` — thin facade over [`process::Supervisor`] that
//! presents the channel-plugin-specific public API consumed by the
//! Dashboard, the REST/WS routes, and the `StateSource` trait.
//!
//! All real lifecycle work (spawn, kill, restart, watchdog, status
//! broadcast) now lives in `process::Supervisor`. This module owns:
//!
//! - The mapping from the configured `channel_instance_id` (`"feishu-work"`)
//!   to the opaque `ProcessId` returned by the supervisor.
//! - The conversion from the generic `ProcessSnapshot` into the
//!   Dashboard-facing `ChannelStatusSnapshot` (status string, reason,
//!   plugin version).
//! - A small forwarder task that fans the supervisor's typed
//!   `ProcessEvent` stream down to the `()` channel that
//!   [`StateSource::subscribe_changes`] exposes.
//!
//! Legacy call sites (`ChannelBridgeHandler`, `StdioPluginRuntime`) still
//! refer to `ChannelMonitor` by name — this file keeps their surface stable.
//!
//! [`StateSource::subscribe_changes`]: crate::state::StateSource::subscribe_changes

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Weak};
use std::time::Duration;

use tokio::sync::{broadcast, mpsc, oneshot};

use super::manifest::ChannelPluginManifest;
use super::plugin_host::PluginHost;
use super::plugin_runner::ChannelPluginRunnerFactory;
use super::{ChannelInput, ConversationIngress};
use crate::process::registry::ProcessKind;
use crate::process::supervisor::{ProcessEvent, ProcessId, RestartPolicy, SpawnSpec, Supervisor};
use crate::routing::{ChannelInstanceId, ChannelKind};

// ---------------------------------------------------------------------------
// Tunables for channel plugin self-recovery.
// ---------------------------------------------------------------------------

pub const RESTART_DELAY: Duration = Duration::from_secs(5);
pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(90);

// ---------------------------------------------------------------------------
// Public status types — shape preserved from pre-migration code so the
// REST handlers + api_types shim don't need to change.
// ---------------------------------------------------------------------------

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelRunStatus {
    NotStarted = 0,
    Spawning = 1,
    Running = 2,
    Crashed = 3,
    Stopped = 4,
}

impl ChannelRunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::Spawning => "spawning",
            Self::Running => "running",
            Self::Crashed => "crashed",
            Self::Stopped => "stopped",
        }
    }

    fn from_process(status: crate::process::supervisor::ProcessStatus) -> Self {
        use crate::process::supervisor::ProcessStatus as P;
        match status {
            P::NotStarted => Self::NotStarted,
            P::Spawning => Self::Spawning,
            P::Running => Self::Running,
            P::Crashed => Self::Crashed,
            P::Stopped => Self::Stopped,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChannelStatusSnapshot {
    pub instance_id: ChannelInstanceId,
    pub kind: String,
    pub version: Option<String>,
    pub plugin_dir: Option<PathBuf>,
    pub status: ChannelRunStatus,
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Facade
// ---------------------------------------------------------------------------

pub struct ChannelMonitor {
    command_tx: mpsc::UnboundedSender<ChannelMonitorCommand>,
    /// Republished `()` stream for `StateSource::subscribe_changes`.
    change_tx: broadcast::Sender<()>,
}

struct ChannelMonitorOwner {
    supervisor: Arc<Supervisor>,
    registrations: HashMap<ChannelInstanceId, ChannelRegistration>,
    ingress: Arc<ConversationIngress>,
    input_tx: mpsc::UnboundedSender<ChannelInput>,
    plugin_host: Arc<PluginHost>,
    change_tx: broadcast::Sender<()>,
}

enum ChannelMonitorCommand {
    Register {
        manifest: Box<ChannelPluginManifest>,
        reply: oneshot::Sender<bool>,
    },
    Touch(ChannelInstanceId),
    ForceStop {
        instance_id: ChannelInstanceId,
        reply: oneshot::Sender<Result<(), String>>,
    },
    Unregister {
        instance_id: ChannelInstanceId,
        reply: oneshot::Sender<Result<(), String>>,
    },
    ForceRestart {
        instance_id: ChannelInstanceId,
        reply: oneshot::Sender<Result<(), String>>,
    },
    ForceStart {
        instance_id: ChannelInstanceId,
        reply: oneshot::Sender<Result<(), String>>,
    },
    Snapshot(oneshot::Sender<Vec<ChannelStatusSnapshot>>),
    RegisteredInstances(oneshot::Sender<Vec<ChannelInstanceId>>),
    Shutdown(oneshot::Sender<()>),
}

#[derive(Debug, Clone)]
struct ChannelRegistration {
    process_id: ProcessId,
    channel_kind: ChannelKind,
    version: String,
    plugin_dir: PathBuf,
}

impl ChannelMonitor {
    /// Build the monitor, spawn the supervisor's tick loop, and start a
    /// forwarder that maps supervisor `ProcessEvent`s (filtered to
    /// `ChannelPlugin`) into the `()` notifications that consumers
    /// subscribe to via [`StateSource::subscribe_changes`].
    ///
    /// [`StateSource::subscribe_changes`]: crate::state::StateSource::subscribe_changes
    pub fn new(
        ingress: Arc<ConversationIngress>,
        input_tx: mpsc::UnboundedSender<ChannelInput>,
        plugin_host: Arc<PluginHost>,
        change_tx: broadcast::Sender<()>,
    ) -> Arc<Self> {
        let supervisor = Supervisor::global();

        let forwarder_rx = supervisor.subscribe();
        let forwarder_tx = change_tx.clone();
        tokio::spawn(forward_events(forwarder_rx, forwarder_tx));

        let (command_tx, command_rx) = mpsc::unbounded_channel();
        tokio::spawn(
            ChannelMonitorOwner {
                supervisor,
                registrations: HashMap::new(),
                ingress,
                input_tx,
                plugin_host,
                change_tx: change_tx.clone(),
            }
            .run(command_rx),
        );
        Arc::new(Self {
            command_tx,
            change_tx,
        })
    }

    /// Register a channel plugin. The supervisor spawns immediately
    /// (no wait for the next tick) and keeps it alive under an
    /// `OnCrash` policy with a short fixed restart delay and a 90-second
    /// heartbeat watchdog.
    pub async fn register(self: &Arc<Self>, manifest: ChannelPluginManifest) -> bool {
        let (reply, registered) = oneshot::channel();
        if self
            .command_tx
            .send(ChannelMonitorCommand::Register {
                manifest: Box::new(manifest),
                reply,
            })
            .is_err()
        {
            return false;
        }
        registered.await.unwrap_or(false)
    }

    /// Bump the liveness timestamp — called on every `_va/heartbeat`
    /// from the plugin. No-op if the channel was deregistered mid-flight.
    pub fn touch(&self, instance_id: &str) {
        let _ = self
            .command_tx
            .send(ChannelMonitorCommand::Touch(instance_id.to_string()));
    }

    pub async fn force_stop(self: &Arc<Self>, instance_id: &str) -> Result<(), String> {
        self.lifecycle_request(instance_id, |instance_id, reply| {
            ChannelMonitorCommand::ForceStop { instance_id, reply }
        })
        .await
    }

    /// Stop and remove a configured instance from both the supervisor and
    /// the monitor registry. Used when settings delete or rename an instance.
    pub async fn unregister(&self, instance_id: &str) -> Result<(), String> {
        self.lifecycle_request(instance_id, |instance_id, reply| {
            ChannelMonitorCommand::Unregister { instance_id, reply }
        })
        .await
    }

    pub async fn force_restart(self: &Arc<Self>, instance_id: &str) -> Result<(), String> {
        self.lifecycle_request(instance_id, |instance_id, reply| {
            ChannelMonitorCommand::ForceRestart { instance_id, reply }
        })
        .await
    }

    pub async fn force_start(self: &Arc<Self>, instance_id: &str) -> Result<(), String> {
        self.lifecycle_request(instance_id, |instance_id, reply| {
            ChannelMonitorCommand::ForceStart { instance_id, reply }
        })
        .await
    }

    pub async fn snapshot(&self) -> Vec<ChannelStatusSnapshot> {
        let (reply, snapshot) = oneshot::channel();
        if self
            .command_tx
            .send(ChannelMonitorCommand::Snapshot(reply))
            .is_err()
        {
            return Vec::new();
        }
        snapshot.await.unwrap_or_default()
    }

    pub async fn registered_instances(&self) -> Vec<ChannelInstanceId> {
        let (reply, instances) = oneshot::channel();
        if self
            .command_tx
            .send(ChannelMonitorCommand::RegisteredInstances(reply))
            .is_err()
        {
            return Vec::new();
        }
        instances.await.unwrap_or_default()
    }

    pub async fn status(&self, instance_id: &str) -> Option<ChannelRunStatus> {
        self.snapshot()
            .await
            .into_iter()
            .find(|snapshot| snapshot.instance_id == instance_id)
            .map(|snapshot| snapshot.status)
    }

    pub fn subscribe_changes(&self) -> broadcast::Receiver<()> {
        self.change_tx.subscribe()
    }

    /// Stop and unregister only the channel processes owned by this monitor.
    /// The process-wide supervisor also serves ACP agents and search, so a
    /// channel facade must never drain its global table.
    pub async fn shutdown_all(&self) {
        let (reply, done) = oneshot::channel();
        if self
            .command_tx
            .send(ChannelMonitorCommand::Shutdown(reply))
            .is_ok()
        {
            let _ = done.await;
        }
    }

    async fn lifecycle_request(
        &self,
        instance_id: &str,
        command: impl FnOnce(
            ChannelInstanceId,
            oneshot::Sender<Result<(), String>>,
        ) -> ChannelMonitorCommand,
    ) -> Result<(), String> {
        let (reply, result) = oneshot::channel();
        self.command_tx
            .send(command(instance_id.to_string(), reply))
            .map_err(|_| "channel monitor stopped".to_string())?;
        result
            .await
            .map_err(|_| "channel monitor stopped".to_string())?
    }
}

impl ChannelMonitorOwner {
    async fn run(mut self, mut command_rx: mpsc::UnboundedReceiver<ChannelMonitorCommand>) {
        while let Some(command) = command_rx.recv().await {
            match command {
                ChannelMonitorCommand::Register { manifest, reply } => {
                    let _ = reply.send(self.register(*manifest).await);
                }
                ChannelMonitorCommand::Touch(instance_id) => self.touch(&instance_id),
                ChannelMonitorCommand::ForceStop { instance_id, reply } => {
                    let result = match self.process_id(&instance_id) {
                        Ok(id) => self
                            .supervisor
                            .force_stop(id)
                            .await
                            .map_err(|error| error.to_string()),
                        Err(error) => Err(error),
                    };
                    let _ = reply.send(result);
                }
                ChannelMonitorCommand::Unregister { instance_id, reply } => {
                    let _ = reply.send(self.unregister(&instance_id).await);
                }
                ChannelMonitorCommand::ForceRestart { instance_id, reply } => {
                    let result = match self.process_id(&instance_id) {
                        Ok(id) => self
                            .supervisor
                            .force_restart(id)
                            .await
                            .map_err(|error| error.to_string()),
                        Err(error) => Err(error),
                    };
                    let _ = reply.send(result);
                }
                ChannelMonitorCommand::ForceStart { instance_id, reply } => {
                    let result = match self.process_id(&instance_id) {
                        Ok(id) => self
                            .supervisor
                            .force_start(id)
                            .await
                            .map_err(|error| error.to_string()),
                        Err(error) => Err(error),
                    };
                    let _ = reply.send(result);
                }
                ChannelMonitorCommand::Snapshot(reply) => {
                    let _ = reply.send(self.snapshot());
                }
                ChannelMonitorCommand::RegisteredInstances(reply) => {
                    let mut instances = self.registrations.keys().cloned().collect::<Vec<_>>();
                    instances.sort();
                    let _ = reply.send(instances);
                }
                ChannelMonitorCommand::Shutdown(reply) => {
                    self.shutdown_all().await;
                    let _ = reply.send(());
                }
            }
        }
    }

    async fn register(&mut self, manifest: ChannelPluginManifest) -> bool {
        let kind = manifest.channel_kind.clone();
        let instance_id = manifest.instance_id.clone();
        if self.registrations.contains_key(&instance_id) {
            tracing::warn!(
                channel_instance = %instance_id,
                channel_kind = %kind,
                "refusing duplicate channel instance registration"
            );
            return false;
        }

        let runtime_dirs = match super::plugin_paths::prepare_plugin_runtime_dirs(&instance_id) {
            Ok(dirs) => dirs,
            Err(error) => {
                tracing::error!(
                    channel_instance = %instance_id,
                    error = %error,
                    "failed to prepare private plugin runtime directories"
                );
                return false;
            }
        };
        let spec = SpawnSpec::new("node")
            .arg(manifest.entry_path.to_string_lossy().to_string())
            .cwd(manifest.plugin_dir.clone())
            .env(
                super::plugin_paths::PLUGIN_STATE_DIR_ENV,
                runtime_dirs.state.to_string_lossy(),
            );
        let factory = ChannelPluginRunnerFactory::new(
            manifest.clone(),
            self.input_tx.clone(),
            Arc::clone(&self.ingress),
            Arc::clone(&self.plugin_host),
        )
        .into_bridge_factory();
        let process_id = self
            .supervisor
            .register(
                ProcessKind::ChannelPlugin,
                instance_id.clone(),
                spec,
                RestartPolicy::OnCrash {
                    restart_delay: RESTART_DELAY,
                    watchdog: Some(HEARTBEAT_TIMEOUT),
                },
                factory,
            )
            .await;
        self.registrations.insert(
            instance_id,
            ChannelRegistration {
                process_id,
                channel_kind: kind,
                version: manifest.version,
                plugin_dir: manifest.plugin_dir,
            },
        );
        let _ = self.change_tx.send(());
        true
    }

    fn touch(&self, instance_id: &str) {
        if let Ok(process_id) = self.process_id(instance_id) {
            self.supervisor.touch(process_id);
        }
    }

    async fn unregister(&mut self, instance_id: &str) -> Result<(), String> {
        let process_id = self.process_id(instance_id)?;
        self.supervisor
            .unregister(process_id)
            .await
            .map_err(|error| error.to_string())?;
        self.registrations.remove(instance_id);
        // The supervisor publishes Stopped before this facade removes its
        // metadata. Notify again so WS snapshots drop the deleted row.
        let _ = self.change_tx.send(());
        Ok(())
    }

    fn snapshot(&self) -> Vec<ChannelStatusSnapshot> {
        self.supervisor
            .snapshot()
            .into_iter()
            .filter(|process| process.kind == ProcessKind::ChannelPlugin)
            .filter_map(|process| {
                let registration = self.registrations.get(&process.label)?;
                Some(ChannelStatusSnapshot {
                    instance_id: process.label,
                    kind: registration.channel_kind.clone(),
                    version: Some(registration.version.clone()),
                    plugin_dir: Some(registration.plugin_dir.clone()),
                    status: ChannelRunStatus::from_process(process.status),
                    reason: process.reason,
                })
            })
            .collect()
    }

    async fn shutdown_all(&mut self) {
        let registrations = std::mem::take(&mut self.registrations);
        for (instance_id, registration) in registrations {
            if let Err(error) = self.supervisor.unregister(registration.process_id).await {
                tracing::warn!(channel_instance = %instance_id, error = %error, "failed to stop channel plugin");
            }
        }
    }

    fn process_id(&self, instance_id: &str) -> Result<ProcessId, String> {
        self.registrations
            .get(instance_id)
            .map(|registration| registration.process_id)
            .ok_or_else(|| format!("unknown channel instance: {instance_id}"))
    }
}

// ---------------------------------------------------------------------------
// StateSource impl (unchanged contract)
// ---------------------------------------------------------------------------

impl crate::state::StateSource for ChannelMonitor {
    type Entry = ChannelStatusSnapshot;

    async fn list(&self) -> Vec<Self::Entry> {
        self.snapshot().await
    }

    fn subscribe_changes(&self) -> broadcast::Receiver<()> {
        self.change_tx.subscribe()
    }
}

// ---------------------------------------------------------------------------
// Weak-ref touch helper — called from the plugin bridge's `_va/heartbeat`
// handler. `mark_crashed_weak` is gone: the supervisor observes the bridge's
// `BridgeExit` directly and no longer needs a weak back-pointer.
// ---------------------------------------------------------------------------

pub fn touch_weak(weak: &Weak<ChannelMonitor>, instance_id: &str) {
    if let Some(monitor) = weak.upgrade() {
        monitor.touch(instance_id);
    }
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// Forwarder that republishes supervisor events as `()` pings, filtered
/// to `ChannelPlugin` entries so the Dashboard WS doesn't re-render for
/// unrelated PTY / tunnel state.
async fn forward_events(mut rx: broadcast::Receiver<ProcessEvent>, tx: broadcast::Sender<()>) {
    loop {
        match rx.recv().await {
            Ok(event) if event.kind == ProcessKind::ChannelPlugin => {
                let _ = tx.send(());
            }
            Ok(_) => {}
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}
