//! Tunnels module: expose the web dashboard over the internet via a public URL.
//! Each provider (Localtunnel, Ngrok, Cloudflare, Tailscale) implements TunnelBackend for unified management and dispatch.

use std::sync::Arc;

use async_trait::async_trait;

pub mod manager;
mod providers;
pub mod status;

pub use manager::{TunnelInfo, TunnelManager};
pub use status::{TunnelMeta, TunnelStatus};

/// Reports the one interactive setup step a tunnel backend may require.
pub type TunnelApprovalReporter = Arc<dyn Fn(String) + Send + Sync>;

/// Tunnel provider: localtunnel, ngrok, cloudflare, or tailscale.
#[derive(Debug, Clone, Copy, Default)]
pub enum TunnelProvider {
    #[default]
    None,
    Localtunnel,
    Ngrok,
    Cloudflare,
    Tailscale,
}

impl TunnelProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            TunnelProvider::None => "none",
            TunnelProvider::Localtunnel => "localtunnel",
            TunnelProvider::Ngrok => "ngrok",
            TunnelProvider::Cloudflare => "cloudflare",
            TunnelProvider::Tailscale => "tailscale",
        }
    }

    /// Returns true if this provider actually creates a tunnel.
    pub fn is_enabled(&self) -> bool {
        !matches!(self, TunnelProvider::None)
    }

    /// Parse from config string (e.g. from settings.json "tunnel.provider").
    pub fn from_config(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "ngrok" => TunnelProvider::Ngrok,
            "cloudflare" => TunnelProvider::Cloudflare,
            "tailscale" => TunnelProvider::Tailscale,
            "localtunnel" => TunnelProvider::Localtunnel,
            _ => TunnelProvider::None,
        }
    }

    /// Return the backend for this provider (for unified start_web_tunnel).
    fn backend(&self) -> Option<&'static dyn TunnelBackend> {
        match self {
            TunnelProvider::None => Option::None,
            TunnelProvider::Localtunnel => Some(&providers::localtunnel::LocaltunnelBackend),
            TunnelProvider::Ngrok => Some(&providers::ngrok::NgrokBackend),
            TunnelProvider::Cloudflare => Some(&providers::cloudflare::CloudflareBackend),
            TunnelProvider::Tailscale => Some(&providers::tailscale::TailscaleBackend),
        }
    }
}

/// Common tunnel-provider interface.
#[async_trait]
pub trait TunnelBackend: Send + Sync {
    /// Provider id (e.g. "localtunnel", "ngrok") for config and logging.
    fn name(&self) -> &'static str;

    /// Start the web tunnel; config supplies credentials and options. Caller keeps the guard and awaits wait().
    async fn start_web_tunnel(
        &self,
        config: &crate::config::Config,
        approval_reporter: Option<TunnelApprovalReporter>,
    ) -> Result<(TunnelGuard, String), Box<dyn std::error::Error + Send + Sync>>;
}

/// Guard that keeps the tunnel alive. Await `wait()` until the tunnel
/// is done (process exit / SDK session closed).
///
/// The process variant holds a `ChildRegistry` id rather than the `Child`
/// directly. `wait()` pops the `Child` back out so `kill_on_drop` still
/// fires on task abort (the `Child` lives in the stack frame of the
/// wait-in-progress future), while daemon shutdown's
/// `ChildRegistry::kill_all()` covers the case where the task is
/// cancelled before even reaching `wait()`.
pub enum TunnelGuard {
    /// Tunnel is a child process (e.g. localtunnel, cloudflared).
    /// The child itself lives in the global `ChildRegistry`.
    Process { registry_id: u64 },
    /// Tunnel is held by an SDK task (e.g. ngrok Rust SDK).
    Sdk(tokio::task::JoinHandle<()>),
}

impl TunnelGuard {
    /// Registry id for the child, if any. Propagated to `TunnelManager`
    /// so `kill()` can SIGKILL on demand even if the owning task is
    /// cancelled before it reaches `wait()`.
    pub fn registry_id(&self) -> Option<u64> {
        match self {
            TunnelGuard::Process { registry_id } => Some(*registry_id),
            TunnelGuard::Sdk(_) => None,
        }
    }

    /// Wait until the tunnel exits. For `Process`, pops the `Child` out
    /// of the registry and awaits its exit; if the task is aborted
    /// mid-wait, the `Child` is dropped and `kill_on_drop` fires.
    pub async fn wait(self) {
        match self {
            TunnelGuard::Process { registry_id } => {
                if let Some(mut child) =
                    crate::process::registry::ChildRegistry::global().remove(registry_id)
                {
                    let _ = child.wait().await;
                }
            }
            TunnelGuard::Sdk(handle) => {
                let _ = handle.await;
            }
        }
    }
}

/// Start the web tunnel using the default provider (Localtunnel) and global config.
/// Returns (guard, public URL). Caller must keep the guard and await `guard.wait()` to keep the tunnel alive.
pub async fn start_web_tunnel(
) -> Result<(TunnelGuard, String), Box<dyn std::error::Error + Send + Sync>> {
    let config = crate::config::ensure_loaded();
    start_web_tunnel_with_provider(TunnelProvider::default(), &config, None).await
}

/// Start the web tunnel with the given provider and config (unified dispatch via TunnelBackend).
pub async fn start_web_tunnel_with_provider(
    provider: TunnelProvider,
    config: &crate::config::Config,
    approval_reporter: Option<TunnelApprovalReporter>,
) -> Result<(TunnelGuard, String), Box<dyn std::error::Error + Send + Sync>> {
    match provider.backend() {
        Some(backend) => backend.start_web_tunnel(config, approval_reporter).await,
        Option::None => Err("Tunnel provider is 'none' — no tunnel to start".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::TunnelProvider;

    #[test]
    fn parses_tailscale_provider() {
        assert_eq!(
            TunnelProvider::from_config("tailscale").as_str(),
            "tailscale"
        );
    }
}
