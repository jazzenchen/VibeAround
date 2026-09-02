//! Tunnels module: expose the web dashboard over the internet via a public URL.
//!
//! Every provider is one supervised child process: [`launch`] builds the
//! provider's [`TunnelPlan`] (what to spawn + how its public URL becomes
//! known), registers it with the process
//! [`Supervisor`](crate::process::Supervisor), and a
//! [`bridge::TunnelBridge`] parses the URL / approval link from stdout. A
//! watcher marks the tunnel failed when the supervisor reports the process
//! stopped — covering crashes and spawn failures alike.

use std::sync::Arc;

mod bridge;
pub mod manager;
mod providers;
pub mod status;

pub use manager::{TunnelInfo, TunnelManager};
pub use status::{TunnelMeta, TunnelStatus};

use crate::process::supervisor::{ProcessStatus, RestartPolicy, SpawnSpec, Supervisor};
use crate::process::ProcessKind;

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
}

/// Launch recipe for a process-based tunnel provider: what to spawn and
/// how its public URL becomes known.
pub(crate) struct TunnelPlan {
    pub(crate) spec: SpawnSpec,
    pub(crate) url: bridge::UrlDiscovery,
}

/// Start the configured tunnel and wire it into `manager`.
///
/// The manager entry must already be registered (so the dashboard shows
/// the tunnel as starting). Returns right after the supervisor accepts the
/// child — the public URL arrives asynchronously via the bridge.
pub async fn launch(
    provider: TunnelProvider,
    config: &crate::config::Config,
    manager: Arc<TunnelManager>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let key = provider.as_str();
    match provider {
        TunnelProvider::None => Err("Tunnel provider is 'none' — no tunnel to start".into()),
        TunnelProvider::Cloudflare
        | TunnelProvider::Localtunnel
        | TunnelProvider::Ngrok
        | TunnelProvider::Tailscale => {
            let plan = match provider {
                TunnelProvider::Cloudflare => providers::cloudflare::plan(config)?,
                TunnelProvider::Localtunnel => providers::localtunnel::plan(config)?,
                TunnelProvider::Ngrok => providers::ngrok::plan(config)?,
                TunnelProvider::Tailscale => providers::tailscale::plan()?,
                TunnelProvider::None => unreachable!("handled above"),
            };

            let supervisor = Supervisor::global();
            let mut events = supervisor.subscribe();
            let mut staged = Some(bridge::TunnelBridge {
                provider_key: key,
                manager: Arc::clone(&manager),
                url: plan.url,
            });
            let factory: crate::process::BridgeFactory = Box::new(move || {
                Box::new(
                    staged
                        .take()
                        .expect("tunnel bridge factory called once (RestartPolicy::Never)"),
                )
            });
            let process_id = supervisor
                .register(
                    ProcessKind::Tunnel,
                    key,
                    plan.spec,
                    RestartPolicy::Never,
                    factory,
                )
                .await;
            manager.bind_supervised(key, process_id);

            // Single exit notifier: `Stopped` under `RestartPolicy::Never` is
            // terminal, whether the child crashed, exited, or never spawned.
            // `TunnelMeta::fail` keeps an earlier terminal status, so a
            // user-killed tunnel (entry already removed) stays untouched.
            let watcher_manager = manager;
            let watcher_supervisor = Arc::clone(&supervisor);
            tokio::spawn(async move {
                loop {
                    match events.recv().await {
                        Ok(event) if event.id == process_id => {
                            if matches!(event.status, ProcessStatus::Stopped) {
                                let reason = if event.reason.is_empty() {
                                    "tunnel process exited".to_string()
                                } else {
                                    event.reason
                                };
                                watcher_manager.set_failed(key, reason);
                                break;
                            }
                        }
                        Ok(_) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            // Dropped events may include our `Stopped`; a
                            // `Never` process leaves the snapshot when it
                            // stops, so absence is the terminal signal.
                            let gone = !watcher_supervisor
                                .snapshot()
                                .iter()
                                .any(|process| process.id == process_id);
                            if gone {
                                watcher_manager
                                    .set_failed(key, "tunnel process exited".to_string());
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
            Ok(())
        }
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
