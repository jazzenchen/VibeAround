//! `TunnelStatus` + `TunnelMeta` — runtime state for a registered tunnel.
//!
//! `TunnelStatus` serializes as a tagged JSON object with a `state`
//! discriminant so consumers pattern-match exhaustively:
//!
//! ```json
//! { "state": "running" }
//! { "state": "awaiting_approval", "url": "https://login.tailscale.com/f/funnel?..." }
//! { "state": "stopped", "reason": "killed" }
//! { "state": "failed",  "error":  "spawn failed" }
//! ```
//!
//! Reference zod schema:
//! `src/shared/client-ts/src/schemas.ts::TunnelStatusSchema`.

use std::sync::Arc;

use parking_lot::RwLock;
use serde::Serialize;

fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Runtime status of a tunnel. Wire-compatible via the `state` tag.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TunnelStatus {
    Running,
    AwaitingApproval { url: String },
    Stopped { reason: String },
    Failed { error: String },
}

impl TunnelStatus {
    pub fn is_running(&self) -> bool {
        matches!(
            self,
            TunnelStatus::Running | TunnelStatus::AwaitingApproval { .. }
        )
    }
}

/// Runtime metadata attached to each tunnel entry: status and start
/// timestamp. Killing lives on the manager, which owns the process /
/// SDK-task handle.
pub struct TunnelMeta {
    pub status: Arc<RwLock<TunnelStatus>>,
    pub started_at: u64,
}

impl Default for TunnelMeta {
    fn default() -> Self {
        Self::new()
    }
}

impl TunnelMeta {
    pub fn new() -> Self {
        Self {
            status: Arc::new(RwLock::new(TunnelStatus::Running)),
            started_at: unix_now_secs(),
        }
    }

    pub fn current_status(&self) -> TunnelStatus {
        self.status.read().clone()
    }

    pub fn uptime_secs(&self) -> u64 {
        unix_now_secs().saturating_sub(self.started_at)
    }

    pub fn stopped(&self, reason: &str) {
        *self.status.write() = TunnelStatus::Stopped {
            reason: reason.to_string(),
        };
    }

    /// Record a failure. A tunnel that is already stopped or failed keeps
    /// its first terminal status — the supervisor-event watcher and the
    /// launch error path can both report without clobbering each other.
    pub fn fail(&self, error: String) {
        let mut status = self.status.write();
        if status.is_running() {
            *status = TunnelStatus::Failed { error };
        }
    }

    pub fn await_approval(&self, url: String) {
        *self.status.write() = TunnelStatus::AwaitingApproval { url };
    }

    pub fn running(&self) {
        *self.status.write() = TunnelStatus::Running;
    }
}

#[cfg(test)]
mod tests {
    use super::{TunnelMeta, TunnelStatus};

    #[test]
    fn records_failed_status() {
        let meta = TunnelMeta::new();
        meta.fail("setup failed".to_string());

        assert!(matches!(
            meta.current_status(),
            TunnelStatus::Failed { error } if error == "setup failed"
        ));
    }

    #[test]
    fn records_awaiting_approval_status() {
        let meta = TunnelMeta::new();
        meta.await_approval("https://login.tailscale.com/f/funnel?node=abc".to_string());

        assert!(matches!(
            meta.current_status(),
            TunnelStatus::AwaitingApproval { url }
                if url == "https://login.tailscale.com/f/funnel?node=abc"
        ));

        meta.running();
        assert!(matches!(meta.current_status(), TunnelStatus::Running));
    }
}
