//! Unified service manager: hierarchical registry for all spawned services.
//!
//! Sub-registries (all DashMap for uniform style):
//! - `pty`: PTY sessions (reuses existing SessionContext)
//! - `tunnel`: tunnel process (at most one entry)
//! - `agents`: agent backends (multiple, keyed by "kind:workspace_path")
//! - `im_bots`: IM bots (keyed by channel kind, e.g. "telegram", "feishu")
//!
//! Each entry carries a `ServiceMeta` with status, start time, and an optional abort handle
//! for killing the associated tokio task.

use std::path::PathBuf;
use std::sync::Arc;

use dashmap::DashMap;
use serde::Serialize;
use tokio::task::AbortHandle;

use crate::agent::AgentKind;
use crate::im::ImChannelKind;
use crate::session::{unix_now_secs, Registry};
use crate::tunnels::TunnelProvider;

// ---------------------------------------------------------------------------
// ServiceStatus + ServiceMeta (shared by all entry types)
// ---------------------------------------------------------------------------

/// Runtime status of a managed service.
#[derive(Debug, Clone)]
pub enum ServiceStatus {
    Running,
    Stopped { reason: String },
    Failed { error: String },
}

impl ServiceStatus {
    pub fn is_running(&self) -> bool {
        matches!(self, ServiceStatus::Running)
    }
}

/// Common metadata shared by all service entry types.
pub struct ServiceMeta {
    pub status: Arc<std::sync::RwLock<ServiceStatus>>,
    pub started_at: u64,
    /// Kill function — aborts the backing task. Works with both tokio and Tauri runtimes.
    kill_fn: Option<Box<dyn Fn() + Send + Sync>>,
}

impl ServiceMeta {
    /// Create with a tokio AbortHandle.
    pub fn new(abort_handle: Option<AbortHandle>) -> Self {
        let kill_fn: Option<Box<dyn Fn() + Send + Sync>> = abort_handle.map(|h| {
            Box::new(move || h.abort()) as Box<dyn Fn() + Send + Sync>
        });
        Self {
            status: Arc::new(std::sync::RwLock::new(ServiceStatus::Running)),
            started_at: unix_now_secs(),
            kill_fn,
        }
    }

    /// Create with a custom kill function (e.g. for Tauri JoinHandle).
    pub fn with_kill_fn(kill_fn: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            status: Arc::new(std::sync::RwLock::new(ServiceStatus::Running)),
            started_at: unix_now_secs(),
            kill_fn: Some(Box::new(kill_fn)),
        }
    }

    /// Replace the kill function (used when the real handle is available after spawn).
    pub fn set_kill_fn(&mut self, kill_fn: impl Fn() + Send + Sync + 'static) {
        self.kill_fn = Some(Box::new(kill_fn));
    }

    /// Mark this service as stopped.
    pub fn mark_stopped(&self, reason: &str) {
        if let Ok(mut s) = self.status.write() {
            *s = ServiceStatus::Stopped {
                reason: reason.to_string(),
            };
        }
    }

    /// Mark this service as failed.
    pub fn mark_failed(&self, error: &str) {
        if let Ok(mut s) = self.status.write() {
            *s = ServiceStatus::Failed {
                error: error.to_string(),
            };
        }
    }

    /// Kill the backing task.
    pub fn kill(&self) {
        if let Some(ref kill_fn) = self.kill_fn {
            kill_fn();
        }
        self.mark_stopped("killed");
    }

    /// Current status snapshot.
    pub fn current_status(&self) -> ServiceStatus {
        self.status
            .read()
            .map(|s| s.clone())
            .unwrap_or(ServiceStatus::Failed {
                error: "lock poisoned".into(),
            })
    }

    /// Uptime in seconds (0 if not running).
    pub fn uptime_secs(&self) -> u64 {
        if self.current_status().is_running() {
            unix_now_secs().saturating_sub(self.started_at)
        } else {
            0
        }
    }
}

// ---------------------------------------------------------------------------
// Entry types (one per sub-registry)
// ---------------------------------------------------------------------------

/// Tunnel service entry.
pub struct TunnelEntry {
    pub meta: ServiceMeta,
    pub provider: TunnelProvider,
    pub url: Arc<std::sync::RwLock<Option<String>>>,
}

/// IM bot service entry.
pub struct ImBotEntry {
    pub meta: ServiceMeta,
    pub kind: ImChannelKind,
}

/// Agent role within the Manager/Worker hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentRole {
    Manager,
    Worker,
}

/// Agent service entry — an independent agent process.
pub struct AgentEntry {
    pub meta: ServiceMeta,
    pub kind: AgentKind,
    pub workspace: PathBuf,
    pub role: AgentRole,
    /// Handle to send messages to this agent (set after start).
    pub backend: Option<Box<dyn crate::agent::AgentBackend>>,
}

/// Web server metadata (read-only, the server is the host and cannot be killed).
#[derive(Debug, Clone, Serialize)]
pub struct ServerMeta {
    pub started_at: u64,
    pub port: u16,
}

// ---------------------------------------------------------------------------
// ServiceManager
// ---------------------------------------------------------------------------

/// Top-level service manager holding all sub-registries.
pub struct ServiceManager {
    /// PTY sessions — reuses the existing session registry.
    pub pty: Registry,
    /// Tunnel (at most one entry, keyed by provider name e.g. "localtunnel").
    pub tunnel: Arc<DashMap<String, TunnelEntry>>,
    /// Agent backends (keyed by "kind:workspace_path").
    pub agents: Arc<DashMap<String, AgentEntry>>,
    /// IM bots (keyed by channel kind id, e.g. "telegram", "feishu").
    pub im_bots: Arc<DashMap<String, ImBotEntry>>,
    /// Web server metadata.
    pub server_meta: ServerMeta,
    /// Convenience: the port the web server listens on.
    pub port: u16,
    /// Broadcast channel for real-time service status changes.
    pub change_tx: tokio::sync::broadcast::Sender<ServicesSnapshot>,
}

impl ServiceManager {
    /// Create a new ServiceManager with empty registries.
    pub fn new(port: u16) -> Self {
        let (change_tx, _) = tokio::sync::broadcast::channel(16);
        Self {
            pty: Arc::new(DashMap::new()),
            tunnel: Arc::new(DashMap::new()),
            agents: Arc::new(DashMap::new()),
            im_bots: Arc::new(DashMap::new()),
            server_meta: ServerMeta {
                started_at: unix_now_secs(),
                port,
            },
            port,
            change_tx,
        }
    }

    /// Broadcast the current services snapshot to all WebSocket subscribers.
    pub fn notify_change(&self) {
        // Ignore send errors (no subscribers connected).
        let _ = self.change_tx.send(self.list_all());
    }

    // -- Tunnel helpers --

    /// Register a tunnel service with a tokio AbortHandle.
    pub fn register_tunnel(
        &self,
        provider: TunnelProvider,
        abort_handle: AbortHandle,
    ) -> Arc<std::sync::RwLock<Option<String>>> {
        let url = Arc::new(std::sync::RwLock::new(None));
        let entry = TunnelEntry {
            meta: ServiceMeta::new(Some(abort_handle)),
            provider,
            url: Arc::clone(&url),
        };
        self.tunnel.insert(provider.as_str().to_string(), entry);
        eprintln!("[VibeAround][services] registered tunnel: {} (abort_handle)", provider.as_str());
        self.notify_change();
        url
    }

    /// Register a tunnel service with a custom kill function (e.g. Tauri JoinHandle).
    pub fn register_tunnel_with_kill_fn(
        &self,
        provider: TunnelProvider,
        kill_fn: impl Fn() + Send + Sync + 'static,
    ) -> Arc<std::sync::RwLock<Option<String>>> {
        let url = Arc::new(std::sync::RwLock::new(None));
        let entry = TunnelEntry {
            meta: ServiceMeta::with_kill_fn(kill_fn),
            provider,
            url: Arc::clone(&url),
        };
        self.tunnel.insert(provider.as_str().to_string(), entry);
        eprintln!("[VibeAround][services] registered tunnel: {} (kill_fn)", provider.as_str());
        self.notify_change();
        url
    }

    /// Set the tunnel URL once it's known.
    pub fn set_tunnel_url(&self, provider: &str, url: &str) {
        if let Some(entry) = self.tunnel.get(provider) {
            if let Ok(mut u) = entry.url.write() {
                *u = Some(url.to_string());
            }
        }
        eprintln!("[VibeAround][services] tunnel {} URL set: {}", provider, url);
        self.notify_change();
    }

    // -- IM bot helpers --

    /// Register an IM bot service with a tokio AbortHandle.
    pub fn register_im_bot(&self, kind: ImChannelKind, abort_handle: AbortHandle) {
        let entry = ImBotEntry {
            meta: ServiceMeta::new(Some(abort_handle)),
            kind,
        };
        self.im_bots.insert(kind.kind_id().to_string(), entry);
        eprintln!("[VibeAround][services] registered im_bot: {}", kind.kind_id());
        self.notify_change();
    }

    /// Register an IM bot service with a custom kill function (e.g. Tauri JoinHandle).
    pub fn register_im_bot_with_kill_fn(
        &self,
        kind: ImChannelKind,
        kill_fn: impl Fn() + Send + Sync + 'static,
    ) {
        let entry = ImBotEntry {
            meta: ServiceMeta::with_kill_fn(kill_fn),
            kind,
        };
        self.im_bots.insert(kind.kind_id().to_string(), entry);
        eprintln!("[VibeAround][services] registered im_bot: {} (kill_fn)", kind.kind_id());
        self.notify_change();
    }

    // -- Agent helpers --

    /// Build the agent key from kind and workspace path.
    pub fn agent_key(kind: AgentKind, workspace: &std::path::Path) -> String {
        format!("{}:{}", kind, workspace.display())
    }

    /// Register an agent entry (without starting it yet).
    pub fn register_agent(
        &self,
        kind: AgentKind,
        workspace: PathBuf,
        role: AgentRole,
        abort_handle: Option<AbortHandle>,
    ) -> String {
        let key = Self::agent_key(kind, &workspace);
        let entry = AgentEntry {
            meta: ServiceMeta::new(abort_handle),
            kind,
            workspace,
            role,
            backend: None,
        };
        self.agents.insert(key.clone(), entry);
        eprintln!("[VibeAround][services] registered agent: {} (role={:?})", key, role);
        self.notify_change();
        key
    }

    // -- Kill --

    /// Kill a service by category and id. Returns true if found and killed.
    pub fn kill(&self, category: &str, id: &str) -> bool {
        let found = match category {
            "tunnel" => {
                if let Some(entry) = self.tunnel.get(id) {
                    entry.meta.kill();
                    true
                } else {
                    false
                }
            }
            "agents" => {
                if let Some(entry) = self.agents.get(id) {
                    entry.meta.kill();
                    true
                } else {
                    false
                }
            }
            "im_bots" => {
                if let Some(entry) = self.im_bots.get(id) {
                    entry.meta.kill();
                    true
                } else {
                    false
                }
            }
            _ => false,
        };
        if found {
            eprintln!("[VibeAround][services] killed {}/{}", category, id);
            self.notify_change();
        }
        found
    }

    // -- List all (serializable) --

    /// Snapshot of all services for API responses.
    pub fn list_all(&self) -> ServicesSnapshot {
        let tunnel = self
            .tunnel
            .iter()
            .map(|entry| {
                let url = entry
                    .url
                    .read()
                    .ok()
                    .and_then(|u| u.clone());
                ServiceInfo {
                    id: entry.key().clone(),
                    category: "tunnel".into(),
                    name: format!("Tunnel ({})", entry.provider.as_str()),
                    status: status_string(&entry.meta.current_status()),
                    status_detail: status_detail(&entry.meta.current_status()),
                    uptime_secs: entry.meta.uptime_secs(),
                    extra: {
                        let mut m = serde_json::Map::new();
                        m.insert("provider".into(), entry.provider.as_str().into());
                        if let Some(ref u) = url {
                            m.insert("url".into(), u.clone().into());
                        }
                        m
                    },
                }
            })
            .collect();

        let agents = self
            .agents
            .iter()
            .map(|entry| ServiceInfo {
                id: entry.key().clone(),
                category: "agents".into(),
                name: format!("{} @ {}", entry.kind, entry.workspace.display()),
                status: status_string(&entry.meta.current_status()),
                status_detail: status_detail(&entry.meta.current_status()),
                uptime_secs: entry.meta.uptime_secs(),
                extra: {
                    let mut m = serde_json::Map::new();
                    m.insert("kind".into(), entry.kind.to_string().into());
                    m.insert(
                        "workspace".into(),
                        entry.workspace.display().to_string().into(),
                    );
                    m.insert(
                        "role".into(),
                        serde_json::to_value(&entry.role).unwrap_or_default(),
                    );
                    m
                },
            })
            .collect();

        let im_bots = self
            .im_bots
            .iter()
            .map(|entry| ServiceInfo {
                id: entry.key().clone(),
                category: "im_bots".into(),
                name: format!("{} Bot", capitalize(entry.kind.kind_id())),
                status: status_string(&entry.meta.current_status()),
                status_detail: status_detail(&entry.meta.current_status()),
                uptime_secs: entry.meta.uptime_secs(),
                extra: {
                    let mut m = serde_json::Map::new();
                    m.insert("kind".into(), entry.kind.kind_id().into());
                    m
                },
            })
            .collect();

        let pty_count = self.pty.len();

        ServicesSnapshot {
            server: self.server_meta.clone(),
            tunnel,
            agents,
            im_bots,
            pty_session_count: pty_count,
        }
    }
}

// ---------------------------------------------------------------------------
// Serializable snapshot types (for API responses)
// ---------------------------------------------------------------------------

/// Full snapshot of all services, returned by GET /api/services.
#[derive(Debug, Clone, Serialize)]
pub struct ServicesSnapshot {
    pub server: ServerMeta,
    pub tunnel: Vec<ServiceInfo>,
    pub agents: Vec<ServiceInfo>,
    pub im_bots: Vec<ServiceInfo>,
    pub pty_session_count: usize,
}

/// A single service entry in the snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct ServiceInfo {
    pub id: String,
    pub category: String,
    pub name: String,
    /// "running", "stopped", or "failed"
    pub status: String,
    /// Optional detail (e.g. stop reason or error message)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_detail: Option<String>,
    pub uptime_secs: u64,
    /// Category-specific extra fields (e.g. "url" for tunnel, "kind" for agent)
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub fn status_string(s: &ServiceStatus) -> String {
    match s {
        ServiceStatus::Running => "running".into(),
        ServiceStatus::Stopped { .. } => "stopped".into(),
        ServiceStatus::Failed { .. } => "failed".into(),
    }
}

fn status_detail(s: &ServiceStatus) -> Option<String> {
    match s {
        ServiceStatus::Running => None,
        ServiceStatus::Stopped { reason } => Some(reason.clone()),
        ServiceStatus::Failed { error } => Some(error.clone()),
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

// ---------------------------------------------------------------------------
// spawn_service helper — wraps tokio::spawn with auto-registration
// ---------------------------------------------------------------------------

/// Spawn a tokio task and register it as a service. Returns the JoinHandle.
/// The task's status is automatically updated when it completes or panics.
pub fn spawn_service<F>(
    meta_status: Arc<std::sync::RwLock<ServiceStatus>>,
    future: F,
) -> tokio::task::JoinHandle<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let status = meta_status;
    tokio::spawn(async move {
        future.await;
        // Task completed normally — mark stopped
        if let Ok(mut s) = status.write() {
            if s.is_running() {
                *s = ServiceStatus::Stopped {
                    reason: "completed".into(),
                };
            }
        }
    })
}
