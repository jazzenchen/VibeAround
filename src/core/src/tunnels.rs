//! Tunnels module: expose the web dashboard over the internet via a public URL.
//! Each provider (Localtunnel, Ngrok, Cloudflare) implements TunnelBackend for unified management and dispatch.

use async_trait::async_trait;

mod cloudflare;
mod localtunnel;
mod ngrok;

/// Tunnel provider: localtunnel (default), ngrok, or cloudflare.
#[derive(Debug, Clone, Copy, Default)]
#[allow(dead_code)] // Ngrok/Cloudflare for future use
pub enum TunnelProvider {
    #[default]
    Localtunnel,
    Ngrok,
    Cloudflare,
}

impl TunnelProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            TunnelProvider::Localtunnel => "localtunnel",
            TunnelProvider::Ngrok => "ngrok",
            TunnelProvider::Cloudflare => "cloudflare",
        }
    }

    /// Parse from config string (e.g. from settings.json "tunnel.provider").
    pub fn from_config(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "ngrok" => TunnelProvider::Ngrok,
            "cloudflare" => TunnelProvider::Cloudflare,
            _ => TunnelProvider::Localtunnel,
        }
    }

    /// Return the backend for this provider (for unified start_web_tunnel).
    fn backend(&self) -> &'static dyn TunnelBackend {
        match self {
            TunnelProvider::Localtunnel => &localtunnel::LocaltunnelBackend,
            TunnelProvider::Ngrok => &ngrok::NgrokBackend,
            TunnelProvider::Cloudflare => &cloudflare::CloudflareBackend,
        }
    }
}

/// Unified tunnel backend trait: same interface for all providers so we can manage and dispatch uniformly.
#[async_trait]
pub trait TunnelBackend: Send + Sync {
    /// Provider id (e.g. "localtunnel", "ngrok") for config and logging.
    fn name(&self) -> &'static str;

    /// Start the web tunnel; config supplies credentials and options. Caller keeps the guard and awaits wait().
    async fn start_web_tunnel(
        &self,
        config: &crate::config::Config,
    ) -> Result<(TunnelGuard, String), Box<dyn std::error::Error + Send + Sync>>;
}

/// Guard that keeps the tunnel alive. Await `wait()` until the tunnel is done (e.g. process exit or SDK session closed).
pub enum TunnelGuard {
    /// Tunnel is a child process (e.g. localtunnel, or ngrok CLI).
    Process(tokio::process::Child),
    /// Tunnel is held by an SDK task (e.g. ngrok Rust SDK).
    Sdk(tokio::task::JoinHandle<()>),
}

impl TunnelGuard {
    /// Wait until the tunnel exits. For Process, waits for the child; for Sdk, waits for the background task.
    pub async fn wait(self) {
        match self {
            TunnelGuard::Process(mut child) => {
                let _ = child.wait().await;
            }
            TunnelGuard::Sdk(handle) => {
                let _ = handle.await;
            }
        }
    }
}

/// Start the web tunnel using the default provider (Localtunnel) and global config.
/// Returns (guard, public URL). Caller must keep the guard and await `guard.wait()` to keep the tunnel alive.
pub async fn start_web_tunnel() -> Result<(TunnelGuard, String), Box<dyn std::error::Error + Send + Sync>> {
    let config = crate::config::ensure_loaded();
    start_web_tunnel_with_provider(TunnelProvider::default(), config).await
}

/// Start the web tunnel with the given provider and config (unified dispatch via TunnelBackend).
pub async fn start_web_tunnel_with_provider(
    provider: TunnelProvider,
    config: &crate::config::Config,
) -> Result<(TunnelGuard, String), Box<dyn std::error::Error + Send + Sync>> {
    provider.backend().start_web_tunnel(config).await
}

// Re-export Localtunnel-specific API (used when default provider is Localtunnel).
#[allow(unused_imports)] // re-exported for external use (e.g. clients that request tunnel URL)
pub use localtunnel::{
    fetch_tunnel_password,
    ping_tunnel_with_bypass,
    BYPASS_TUNNEL_REMINDER_HEADER,
    BYPASS_TUNNEL_REMINDER_VALUE,
    TUNNEL_BYPASS_USER_AGENT,
    TUNNEL_PASSWORD_URL,
};
