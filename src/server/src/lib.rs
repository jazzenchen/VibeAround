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
    /// 1. Channel plugins (driven by settings.json channels config)
    /// 2. Web server (Axum: HTTP API + WebSocket + SPA)
    /// 3. Tunnel (cloudflare / localtunnel / ngrok)
    pub async fn start(&self, dist_path: PathBuf) -> Result<(), String> {
        // Check if another instance is already running on the same port
        if let Ok(_) = tokio::net::TcpStream::connect(("127.0.0.1", self.port)).await {
            eprintln!(
                "[VibeAround] ⚠️  Another instance is already running on port {}. \
                 The new instance will fail to bind.",
                self.port
            );
        }

        let cfg = config::ensure_loaded();
        let services = &self.services;

        // 1. Channel plugins — start plugins for each channel in settings.json
        for name in cfg.channel_names() {
            let plugin_entry = "dist/main.js";
            let candidates = [
                config::data_dir().join("plugins").join(&name),
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .unwrap_or(std::path::Path::new("."))
                    .join("plugins")
                    .join(&name),
                std::env::current_dir()
                    .unwrap_or_default()
                    .join("src")
                    .join("plugins")
                    .join(&name),
            ];

            let plugin_dir = match candidates.iter().find(|d| d.join(plugin_entry).exists()) {
                Some(d) => d.clone(),
                None => {
                    eprintln!("[VibeAround][daemon] no plugin found for channel '{}', skipping", name);
                    continue;
                }
            };

            let kind = match name.as_str() {
                "feishu" => ImChannelKind::Feishu,
                other => {
                    eprintln!("[VibeAround][daemon] unknown channel kind '{}', skipping", other);
                    continue;
                }
            };

            if let Some(abort_handle) =
                common::im::channels::plugin::run_plugin_bot(plugin_dir, &name, Arc::clone(services)).await
            {
                services.register_im_bot(kind, abort_handle);
            }
        }

        // 2. Web server (Axum)
        let web_services = Arc::clone(services);
        let web_handle = tokio::spawn(async move {
            run_web_server(
                common::config::DEFAULT_PORT,
                dist_path,
                web_services,
            )
            .await
        });

        // 3. Tunnel
        let tunnel_provider = cfg.tunnel_provider;
        eprintln!("[VibeAround][daemon] Tunnel ({})", tunnel_provider.as_str());
        let tunnel_services = Arc::clone(services);
        let tunnel_handle = tokio::spawn(async move {
            match tunnels::start_web_tunnel_with_provider(tunnel_provider, cfg).await {
                Ok((guard, url)) => {
                    eprintln!("[VibeAround][daemon] Tunnel URL: {}", url);
                    tunnel_services.set_tunnel_url(tunnel_provider.as_str(), &url);
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
