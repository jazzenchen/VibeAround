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
use tokio::task::AbortHandle;

use crate::pty::unix_now_secs;

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

/// Runtime metadata attached to each tunnel entry: status, start
/// timestamp, and the abort closure used by `kill`.
pub struct TunnelMeta {
    pub status: Arc<RwLock<TunnelStatus>>,
    pub started_at: u64,
    kill_fn: Option<Box<dyn Fn() + Send + Sync>>,
}

impl TunnelMeta {
    pub fn new(abort_handle: Option<AbortHandle>) -> Self {
        let kill_fn: Option<Box<dyn Fn() + Send + Sync>> =
            abort_handle.map(|h| Box::new(move || h.abort()) as Box<dyn Fn() + Send + Sync>);
        Self {
            status: Arc::new(RwLock::new(TunnelStatus::Running)),
            started_at: unix_now_secs(),
            kill_fn,
        }
    }

    pub fn current_status(&self) -> TunnelStatus {
        self.status.read().clone()
    }

    pub fn uptime_secs(&self) -> u64 {
        unix_now_secs().saturating_sub(self.started_at)
    }

    pub fn kill(&self) {
        if let Some(f) = &self.kill_fn {
            f();
        }
        // Never hold this write guard across an .await — we drop it at end of scope.
        let mut s = self.status.write();
        *s = TunnelStatus::Stopped {
            reason: "killed".into(),
        };
    }

    pub fn fail(&self, error: String) {
        *self.status.write() = TunnelStatus::Failed { error };
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
        let meta = TunnelMeta::new(None);
        meta.fail("setup failed".to_string());

        assert!(matches!(
            meta.current_status(),
            TunnelStatus::Failed { error } if error == "setup failed"
        ));
    }

    #[test]
    fn records_awaiting_approval_status() {
        let meta = TunnelMeta::new(None);
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
