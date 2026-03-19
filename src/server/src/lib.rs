//! VibeAround server crate: Axum HTTP + WebSocket, and the unified ServerDaemon entry point.

mod web_server;

pub use web_server::run_web_server;

use std::path::PathBuf;
use std::sync::Arc;

use common::config;
use common::im::ImChannelKind;
use common::service::ServiceManager;
use common::tunnels;

/// Unified daemon that starts and manages all VibeAround services.
/// Both the server binary and the desktop (Tauri) binary use this.
pub struct ServerDaemon {
    pub services: Arc<ServiceManager>,
    pub port: u16,
}

impl ServerDaemon {
    pub fn new(port: u16) -> Self {
        Self {
            services: Arc::new(ServiceManager::new(port)),
            port,
        }
    }

    /// Get a clone of the ServiceManager Arc (for Tauri state injection, etc.)
    pub fn services(&self) -> Arc<ServiceManager> {
        Arc::clone(&self.services)
    }

    /// Start all services and wait for shutdown (ctrl_c or web server exit).
    ///
    /// Services started:
    /// 1. Feishu IM bot (webhook mode)
    /// 2. Web server (Axum: HTTP API + WebSocket + SPA)
    /// 3. Telegram IM bot
    /// 4. Tunnel (cloudflare / localtunnel / ngrok)
    pub async fn start(&self, dist_path: PathBuf) -> Result<(), String> {
        let cfg = config::ensure_loaded();
        let services = &self.services;

        // 1. Feishu bot (webhook state for web server routes)
        let feishu_state =
            common::im::channels::feishu::run_feishu_bot(Arc::clone(services)).await;
        if feishu_state.is_some() {
            services.register_im_bot_with_kill_fn(ImChannelKind::Feishu, || {
                eprintln!("[VibeAround] Feishu webhook cannot be killed independently");
            });
        }

        // 2. Web server (Axum)
        let web_services = Arc::clone(services);
        let web_handle = tokio::spawn(async move {
            run_web_server(
                common::config::DEFAULT_PORT,
                dist_path,
                feishu_state,
                web_services,
            )
            .await
        });

        // 3. Telegram bot
        if cfg.telegram_bot_token.as_ref().map_or(false, |t| !t.is_empty()) {
            eprintln!("[VibeAround][daemon] Starting Telegram bot");
            let tg_services = Arc::clone(services);
            let tg_handle = tokio::spawn(async move {
                common::im::channels::telegram::run_telegram_bot(tg_services).await;
            });
            services.register_im_bot(ImChannelKind::Telegram, tg_handle.abort_handle());
        }

        // 4. Tunnel
        let tunnel_provider = cfg.tunnel_provider;
        eprintln!("[VibeAround][daemon] Tunnel ({})", tunnel_provider.as_str());
        let tunnel_services = Arc::clone(services);
        let tunnel_handle = tokio::spawn(async move {
            match tunnels::start_web_tunnel_with_provider(tunnel_provider, cfg).await {
                Ok((guard, url)) => {
                    eprintln!("[VibeAround][daemon] Tunnel URL: {}", url);
                    tunnel_services.set_tunnel_url(tunnel_provider.as_str(), &url);
                    // Keep tunnel alive until task is aborted
                    guard.wait().await;
                }
                Err(e) => {
                    eprintln!("[VibeAround][daemon] Tunnel failed: {}", e);
                }
            }
        });
        services.register_tunnel(tunnel_provider, tunnel_handle.abort_handle());

        // Wait for web server or ctrl_c
        tokio::select! {
            result = web_handle => {
                match result {
                    Ok(Ok(())) => eprintln!("[VibeAround][daemon] web server stopped"),
                    Ok(Err(e)) => eprintln!("[VibeAround][daemon] web server error: {}", e),
                    Err(e) => eprintln!("[VibeAround][daemon] web server panic: {}", e),
                }
            }
            _ = tokio::signal::ctrl_c() => {
                eprintln!("\n[VibeAround][daemon] shutting down...");
            }
        }

        Ok(())
    }
}
