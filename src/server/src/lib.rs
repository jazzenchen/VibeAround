//! VibeAround server crate: Axum HTTP + WebSocket, and the unified ServerDaemon entry point.

pub mod api_types;
pub mod boot;
mod web_server;

pub use web_server::run_web_server;

use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use common::auth::{self, AuthToken, SharedAuthToken};
use common::channels::{ChannelManager, WebChannelManager};
use common::config;
use common::plugins;
use common::process::registry::{self as child_registry, ChildRegistry};
use common::search::SearchToolRuntime;
use common::tunnels::{self, TunnelManager};
use common::workspace::WorkspaceThreadManager;

const WEB_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);
#[cfg(windows)]
const WEB_BIND_RETRY_ATTEMPTS: usize = 20;
#[cfg(windows)]
const WEB_BIND_RETRY_DELAY: Duration = Duration::from_millis(150);

/// Unified daemon that starts and manages all VibeAround services.
/// Both the server binary and the desktop (Tauri) binary use this.
pub struct ServerDaemon {
    pub tunnels: Arc<TunnelManager>,
    pub port: u16,
    /// Per-session auth token, regenerated on every daemon start.
    /// Exposed so Tauri can append `?token=` when opening the dashboard.
    pub auth_token: Arc<AuthToken>,
    /// Scoped credential accepted only by the MCP endpoint.
    mcp_token: Arc<AuthToken>,
    /// Scoped credential accepted only by the local API bridge.
    local_api_token: Arc<AuthToken>,
    /// Scoped credential accepted only by the agent-as-API surface.
    ///
    /// Persisted across restarts and rotated only on request — users paste it
    /// into provider profiles by hand.
    local_agent_api_token: SharedAuthToken,
}

pub struct RunningDaemon {
    pub channel_hub: Arc<ChannelManager>,
    pub workspace_thread_manager: Arc<WorkspaceThreadManager>,
    pub web_channel: Arc<WebChannelManager>,
    pub web_handle: JoinHandle<Result<(), String>>,
    pub tunnel_handle: JoinHandle<()>,
    pub web_dispatch_handle: JoinHandle<()>,
    pub search_runtime: Option<Arc<SearchToolRuntime>>,
    pub tunnels: Arc<TunnelManager>,
    /// Signal used to stop the channel-input task.
    channel_input_shutdown: Arc<Notify>,
    /// Signal to Axum so it can stop accepting new connections before the
    /// web task is force-aborted.
    web_shutdown: Arc<Notify>,
    /// Owned so `stop()` can wait for the dispatcher to exit.
    channel_input_handle: JoinHandle<()>,
}

impl RunningDaemon {
    pub async fn stop(self) {
        self.stop_inner(false).await;
    }

    async fn stop_inner(self, web_handle_completed: bool) {
        let RunningDaemon {
            channel_hub,
            workspace_thread_manager,
            web_handle,
            tunnel_handle,
            web_dispatch_handle,
            search_runtime,
            tunnels,
            channel_input_shutdown,
            web_shutdown,
            channel_input_handle,
            ..
        } = self;

        // Close every ingress before tearing down the runtimes it can reach.
        // This ordering prevents a queued route-lane command from spawning a
        // fresh ACP host while daemon shutdown is already reaping children.
        web_shutdown.notify_waiters();
        channel_input_shutdown.notify_waiters();
        channel_input_handle.abort();
        let _ = channel_input_handle.await;
        channel_hub.ingress().shutdown().await;
        channel_hub.shutdown_all().await;
        workspace_thread_manager.shutdown_all().await;
        if let Some(search_runtime) = search_runtime {
            search_runtime.shutdown().await;
        }

        // Safety net: synchronously kill any child process still registered
        // after the graceful shutdown paths ran. Covers cases where the
        // supervisor-driven cancel + kill_on_drop never got polled because
        // the tokio runtime tore down first.
        ChildRegistry::global().kill_all();

        // Stop previewed development servers during daemon shutdown.
        common::previews::cleanup_registered_previews();

        web_dispatch_handle.abort();
        tunnel_handle.abort();

        // Let Axum close the listener cleanly, but do not let long-lived
        // websocket clients hang daemon shutdown forever.
        finish_web_handle(web_handle, web_handle_completed).await;

        // Wait for aborted tasks so their sockets and child handles are
        // dropped before a hot restart probes/binds the same port.
        let _ = tunnel_handle.await;
        let _ = web_dispatch_handle.await;

        // Clear the tunnel registry so stale entries don't persist
        // across restarts.
        tunnels.clear();
    }
}

async fn finish_web_handle(
    mut web_handle: JoinHandle<Result<(), String>>,
    already_completed: bool,
) {
    // Tokio JoinHandle panics if it is polled again after completion. The
    // foreground start path already awaited it in select!, while desktop
    // shutdown reaches this helper with a still-running handle.
    if already_completed {
        return;
    }
    if tokio::time::timeout(WEB_SHUTDOWN_TIMEOUT, &mut web_handle)
        .await
        .is_err()
    {
        web_handle.abort();
        let _ = web_handle.await;
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::warn!(error = %error, "failed to install Ctrl+C handler");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                tracing::warn!(error = %error, "failed to install SIGTERM handler");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("received Ctrl+C"),
        _ = terminate => tracing::info!("received SIGTERM"),
    }
}

async fn bind_web_listener(port: u16) -> anyhow::Result<tokio::net::TcpListener> {
    #[cfg(windows)]
    {
        // During desktop restarts, Windows can keep the old listener handle
        // alive briefly while aborted tasks or killed child processes finish
        // releasing inherited handles. A short Windows-only retry turns that
        // transient "first click fails, second click works" state into a
        // single successful restart without changing macOS/Linux behavior.
        for attempt in 0..WEB_BIND_RETRY_ATTEMPTS {
            match web_server::bind_web_listener(port).await {
                Ok(listener) => return Ok(listener),
                Err(error)
                    if error.kind() == ErrorKind::AddrInUse
                        && attempt + 1 < WEB_BIND_RETRY_ATTEMPTS =>
                {
                    if attempt == 0 {
                        tracing::info!(
                            port,
                            "web server port is still busy after restart; waiting before retry"
                        );
                    }
                    tokio::time::sleep(WEB_BIND_RETRY_DELAY).await;
                }
                Err(error) => return Err(bind_web_listener_error(port, error)),
            }
        }
        unreachable!("web bind retry loop should return");
    }

    #[cfg(not(windows))]
    {
        web_server::bind_web_listener(port)
            .await
            .map_err(|error| bind_web_listener_error(port, error))
    }
}

fn bind_web_listener_error(port: u16, error: std::io::Error) -> anyhow::Error {
    if error.kind() == ErrorKind::AddrInUse {
        anyhow!(
            "Port {} is still in use. Another VibeAround instance or a stale previous service may be holding it.",
            port
        )
    } else {
        anyhow!(error).context(format!("failed to bind web server port {}", port))
    }
}

impl ServerDaemon {
    pub fn new(port: u16) -> Self {
        Self {
            tunnels: TunnelManager::new(),
            port,
            auth_token: Arc::new(AuthToken::generate()),
            mcp_token: Arc::new(AuthToken::generate()),
            local_api_token: Arc::new(AuthToken::generate()),
            local_agent_api_token: SharedAuthToken::new(
                auth::load_or_create_local_agent_api_token(),
            ),
        }
    }

    /// Mint a replacement agent-as-API credential and persist it.
    ///
    /// The previous key stops working immediately, so profiles carrying it
    /// need the new value pasted in.
    pub fn rotate_local_agent_api_token(&self) -> std::io::Result<String> {
        // Persist before swapping: a token the disk never received would be
        // demanded by the running daemon and known to nobody.
        let next = AuthToken::generate();
        self.write_auth_file(&next)?;
        self.local_agent_api_token.replace(next.clone());
        Ok(next.as_str().to_string())
    }

    pub fn tunnels(&self) -> Arc<TunnelManager> {
        Arc::clone(&self.tunnels)
    }

    /// Borrow the session auth token. Tauri uses this to open the dashboard
    /// with a `?token=` query parameter.
    pub fn auth_token(&self) -> Arc<AuthToken> {
        Arc::clone(&self.auth_token)
    }

    /// Write the auth file so out-of-process clients can authenticate without
    /// an IPC round-trip.
    ///
    /// Safe to call before `start_background()` — the file will be
    /// overwritten there too, but the contents are identical, so the early
    /// write avoids a race where the desktop-ui queries the token before
    /// the daemon's start path has finished persisting it.
    pub fn persist_auth_tokens(&self) -> std::io::Result<()> {
        self.write_auth_file(&self.local_agent_api_token.snapshot())
    }

    /// Serialize the whole credential set from memory, never read-modify-write,
    /// so a concurrent write cannot drop a token that is not in this snapshot.
    fn write_auth_file(&self, agent: &AuthToken) -> std::io::Result<()> {
        auth::write_auth_file(
            self.port,
            auth::DaemonTokens {
                dashboard: &self.auth_token,
                mcp: &self.mcp_token,
                bridge: &self.local_api_token,
                agent,
            },
        )
    }

    pub async fn start_background(&self, dist_path: PathBuf) -> anyhow::Result<RunningDaemon> {
        // Reap plugin and ACP children left by a crashed daemon.
        child_registry::orphan_sweep();
        common::previews::cleanup_registered_previews();

        let web_listener = bind_web_listener(self.port).await?;

        // Force a fresh config read on every daemon start — ensures the
        // in-memory cache reflects the latest settings.json (which may have
        // been rewritten by onboarding or a manual edit since last start).
        let cfg = config::reload();
        let tunnels = Arc::clone(&self.tunnels);

        // Rewrite the auth file. Session tokens from the previous run go
        // invalid immediately; the agent-as-API credential is restored, not
        // regenerated, so profiles holding it keep working.
        if let Err(e) = self.persist_auth_tokens() {
            tracing::warn!(
                error = %e,
                "failed to write the auth file — authenticated local clients may be unavailable"
            );
        }

        // 1. Initialize workspace-thread routing and channel hub.
        let workspace_thread_manager = WorkspaceThreadManager::new_default();
        let (channel_hub, mut input_rx) =
            ChannelManager::new(Arc::clone(&workspace_thread_manager));
        let channel_hub = Arc::new(channel_hub);
        let web_channel = WebChannelManager::new();

        // Register built-in internal channels.
        let (web_outbound_tx, mut web_outbound_rx) = web_channel.sender();
        channel_hub.start_internal_plugin("web", web_outbound_tx.clone());
        channel_hub.start_internal_plugin("tui", web_outbound_tx);
        let web_dispatch_handle = {
            let web_channel = Arc::clone(&web_channel);
            tokio::spawn(async move {
                while let Some(output) = web_outbound_rx.recv().await {
                    web_channel.dispatch_output(output).await;
                }
            })
        };

        // ChannelManager owns an input sender, so shutdown uses an explicit signal.
        let conversation_ingress = channel_hub.ingress();
        let channel_input_shutdown = Arc::new(Notify::new());
        let input_shutdown_for_task = Arc::clone(&channel_input_shutdown);
        let channel_input_handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = input_shutdown_for_task.notified() => break,
                    maybe = input_rx.recv() => {
                        let Some(input) = maybe else { break };
                        conversation_ingress.dispatch(input);
                    }
                }
            }
        });

        // 3. Channel plugins — supervised by ChannelMonitor (respawn on
        //    crash + heartbeat watchdog). Handlers reach the monitor
        //    directly via `state.channel_hub.monitor()`; no back-ref
        //    needed.
        let discovered_plugins = plugins::channel::discover();
        for name in cfg.channel_names() {
            let Some(plugin) = discovered_plugins.get(&name) else {
                tracing::warn!(channel = %name, "no plugin found, skipping");
                continue;
            };
            channel_hub.register_plugin(&name, plugin).await;
        }

        // 4. Search provider runtime — supervised like ACP providers. It
        //    starts when at least one host search source is enabled.
        let host_search_available = cfg.search_tool.has_enabled_sources();
        let replace_provider_web_search = cfg.api_bridge.replace_provider_web_search;
        let service_side = cfg.service_side.clone();
        let search_runtime = SearchToolRuntime::spawn_if_enabled(&cfg.search_tool).await?;

        // 5. Web server (Axum)
        let web_tunnels = Arc::clone(&tunnels);
        let web_channel_hub = Arc::clone(&channel_hub);
        let web_channel_manager = Arc::clone(&web_channel);
        let web_auth_token = Arc::clone(&self.auth_token);
        let web_mcp_token = Arc::clone(&self.mcp_token);
        let web_local_api_token = Arc::clone(&self.local_api_token);
        let web_local_agent_api_token = self.local_agent_api_token.clone();
        let web_search_runtime = search_runtime.clone();
        let web_search_available = host_search_available;
        let web_replace_provider_search = replace_provider_web_search;
        let web_shutdown = Arc::new(Notify::new());
        let web_shutdown_for_server = Arc::clone(&web_shutdown);
        let web_handle = tokio::spawn(async move {
            run_web_server(
                web_listener,
                dist_path,
                web_tunnels,
                web_channel_hub,
                web_channel_manager,
                web_auth_token,
                web_mcp_token,
                web_local_api_token,
                web_local_agent_api_token,
                web_search_available,
                web_replace_provider_search,
                service_side,
                web_search_runtime,
                web_shutdown_for_server,
            )
            .await
            .map_err(|e| e.to_string())
        });

        // 6. Tunnel (skip when provider is "none")
        let tunnel_provider = cfg.tunnel_provider;
        tracing::info!(provider = %tunnel_provider.as_str(), "tunnel configured");
        let tunnel_handle = if tunnel_provider.is_enabled() {
            let tunnel_manager = Arc::clone(&tunnels);
            let approval_manager = Arc::clone(&tunnels);
            let approval_reporter: tunnels::TunnelApprovalReporter = Arc::new(move |url| {
                approval_manager.set_awaiting_approval(tunnel_provider.as_str(), url);
            });
            let handle = tokio::spawn(async move {
                match tunnels::start_web_tunnel_with_provider(
                    tunnel_provider,
                    &cfg,
                    Some(approval_reporter),
                )
                .await
                {
                    Ok((guard, url)) => {
                        tracing::info!(url = %url, "tunnel connected");
                        tunnel_manager.set_url(tunnel_provider.as_str(), &url);
                        if let Some(id) = guard.registry_id() {
                            tunnel_manager.set_registry_id(tunnel_provider.as_str(), id);
                        }
                        guard.wait().await;
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "tunnel failed");
                        tunnel_manager.set_failed(tunnel_provider.as_str(), e.to_string());
                    }
                }
            });
            tunnels.register(tunnel_provider, handle.abort_handle());
            handle
        } else {
            tracing::debug!("tunnel disabled (provider=none)");
            tokio::spawn(async { /* no-op: keep the JoinHandle type consistent */ })
        };

        Ok(RunningDaemon {
            channel_hub,
            workspace_thread_manager,
            web_channel,
            web_handle,
            tunnel_handle,
            web_dispatch_handle,
            search_runtime,
            tunnels,
            channel_input_shutdown,
            web_shutdown,
            channel_input_handle,
        })
    }

    pub async fn start(&self, dist_path: PathBuf) -> anyhow::Result<()> {
        let mut running = self.start_background(dist_path).await?;

        let web_handle_completed = tokio::select! {
            result = &mut running.web_handle => {
                match result {
                    Ok(Ok(())) => tracing::info!("web server stopped"),
                    Ok(Err(e)) => tracing::error!(error = %e, "web server error"),
                    Err(e) => tracing::error!(error = %e, "web server panic"),
                }
                running.tunnel_handle.abort();
                true
            }
            _ = shutdown_signal() => {
                tracing::info!("shutting down");
                false
            }
        };

        running.stop_inner(web_handle_completed).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn completed_web_handle_is_not_polled_twice() {
        let mut handle = tokio::spawn(async { Ok(()) });
        (&mut handle).await.unwrap().unwrap();

        finish_web_handle(handle, true).await;
    }
}
