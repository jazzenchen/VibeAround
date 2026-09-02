//! `TunnelManager` — owns the registry of active tunnels.
//!
//! Follows the "per-domain kernel manager + `StateSource` trait" pattern
//! shared with `ChannelMonitor` and `WorkspaceThreadManager`: consumers read tunnel state
//! via `list()` / `subscribe_changes()` directly — there is no aggregate
//! facade above these managers.

use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::broadcast;

use crate::process::supervisor::{ProcessId, Supervisor};

use super::status::{TunnelMeta, TunnelStatus};

use super::TunnelProvider;

/// One registered tunnel (at most one per provider in normal operation).
/// Held internally by `TunnelManager`; external consumers see the
/// value-typed [`TunnelInfo`] via [`TunnelManager::list`].
pub struct TunnelEntry {
    pub meta: TunnelMeta,
    pub provider: TunnelProvider,
    /// Public URL once the backend has finished connecting. `None` while
    /// the backend is still starting up.
    pub url: Option<String>,
    /// The tunnel's supervisor node; `None` for the brief window between
    /// `register` and `bind_supervised`. Killing the tunnel unregisters it.
    process_id: Option<ProcessId>,
}

/// Value-typed view of a single tunnel's current state, suitable for
/// handing out to consumers that iterate the registry. This is what
/// `StateSource::list` returns.
#[derive(Debug, Clone)]
pub struct TunnelInfo {
    pub provider: TunnelProvider,
    pub url: Option<String>,
    pub status: TunnelStatus,
    pub uptime_secs: u64,
}

/// Owner of the live tunnel registry. Wire shells (HTTP, TUI, CLI) use
/// [`StateSource`] to inspect it; internal code that needs to register a
/// newly-spawned tunnel or update its URL calls the mutators directly.
///
/// [`StateSource`]: crate::state::StateSource
pub struct TunnelManager {
    tunnels: DashMap<String, TunnelEntry>,
    change_tx: broadcast::Sender<()>,
}

impl TunnelManager {
    pub fn new() -> Arc<Self> {
        let (change_tx, _) = broadcast::channel(32);
        Arc::new(Self {
            tunnels: DashMap::new(),
            change_tx,
        })
    }

    /// Register a tunnel that is starting up. The launch path binds the
    /// supervisor node once the child is registered.
    pub fn register(&self, provider: TunnelProvider) {
        self.tunnels.insert(
            provider.as_str().to_string(),
            TunnelEntry {
                meta: TunnelMeta::new(),
                provider,
                url: None,
                process_id: None,
            },
        );
        self.notify_change();
    }

    /// Bind a tunnel to its supervisor node.
    pub fn bind_supervised(&self, provider_key: &str, process_id: ProcessId) {
        if let Some(mut entry) = self.tunnels.get_mut(provider_key) {
            entry.process_id = Some(process_id);
        }
    }

    /// Set the public URL once the backend reports it.
    pub fn set_url(&self, provider_key: &str, url: &str) {
        if let Some(mut entry) = self.tunnels.get_mut(provider_key) {
            entry.url = Some(url.to_string());
            entry.meta.running();
        }
        self.notify_change();
    }

    pub fn set_failed(&self, provider_key: &str, error: String) {
        if let Some(entry) = self.tunnels.get(provider_key) {
            entry.meta.fail(error);
        }
        self.notify_change();
    }

    pub fn set_awaiting_approval(&self, provider_key: &str, url: String) {
        if let Some(entry) = self.tunnels.get(provider_key) {
            entry.meta.await_approval(url);
        }
        self.notify_change();
    }

    /// Kill the tunnel matching `provider_key` and remove it from the
    /// registry. Returns `true` if an entry was found and killed.
    pub fn kill(&self, provider_key: &str) -> bool {
        let Some((_, entry)) = self.tunnels.remove(provider_key) else {
            return false;
        };
        entry.meta.stopped("killed");
        if let Some(process_id) = entry.process_id {
            tokio::spawn(async move {
                let _ = Supervisor::global().unregister(process_id).await;
            });
        }
        self.notify_change();
        true
    }

    /// Clear all tunnels. Called on daemon stop, after the supervisor has
    /// stopped the tunnel processes.
    pub fn clear(&self) {
        self.tunnels.clear();
        self.notify_change();
    }

    /// True if any registered tunnel has a URL (i.e. at least one tunnel
    /// is fully up).
    pub fn has_url(&self) -> bool {
        self.tunnels.iter().any(|entry| entry.url.is_some())
    }

    /// First registered public URL.
    pub fn first_url(&self) -> Option<String> {
        self.tunnels.iter().find_map(|entry| entry.url.clone())
    }

    /// Snapshot of every public URL currently registered.
    pub fn public_urls(&self) -> Vec<String> {
        self.tunnels
            .iter()
            .filter_map(|entry| entry.url.clone())
            .collect()
    }

    fn notify_change(&self) {
        let _ = self.change_tx.send(());
    }
}

impl crate::state::StateSource for TunnelManager {
    type Entry = TunnelInfo;

    async fn list(&self) -> Vec<Self::Entry> {
        self.tunnels
            .iter()
            .map(|entry| TunnelInfo {
                provider: entry.provider,
                url: entry.url.clone(),
                status: entry.meta.current_status(),
                uptime_secs: entry.meta.uptime_secs(),
            })
            .collect()
    }

    fn subscribe_changes(&self) -> broadcast::Receiver<()> {
        self.change_tx.subscribe()
    }
}
