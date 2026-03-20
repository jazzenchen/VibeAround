//! VibeAround server crate: Axum HTTP + WebSocket, and the unified ServerDaemon entry point.

mod web_server;

pub use web_server::run_web_server;

use std::path::PathBuf;
use std::sync::Arc;

use common::config;
use common::hub::agent_hub::AgentHub;
use common::hub::channel_hub::ChannelHub;
use common::hub::session_hub::SessionHub;
use common::hub::types::HubEvent;
use common::service::ServiceStatusManager;
use common::tunnels;

/// Unified daemon that starts and manages all VibeAround services.
/// Both the server binary and the desktop (Tauri) binary use this.
pub struct ServerDaemon {
    pub services: Arc<ServiceStatusManager>,
    pub port: u16,
}

impl ServerDaemon {
    pub fn new(port: u16) -> Self {
        Self {
            services: Arc::new(ServiceStatusManager::new(port)),
            port,
        }
    }

    /// Get a clone of the ServiceStatusManager Arc (for Tauri state injection, etc.)
    pub fn services(&self) -> Arc<ServiceStatusManager> {
        Arc::clone(&self.services)
    }

    /// Start all services and wait for shutdown (ctrl_c or web server exit).
    ///
    /// Services started:
    /// 1. Hub architecture (ChannelHub + SessionHub + AgentHub)
    /// 2. Channel plugins (driven by settings.json channels config)
    /// 3. Hub event subscriber (syncs hub state → ServiceStatusManager → Dashboard)
    /// 4. Web server (Axum: HTTP API + WebSocket + SPA)
    /// 5. Tunnel (cloudflare / localtunnel / ngrok)
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

        // 1. Initialize hub architecture (two-phase init to avoid circular Arc)
        let channel_hub = Arc::new(ChannelHub::new());
        let session_hub = Arc::new(SessionHub::new());
        let agent_hub = Arc::new(AgentHub::new());

        // Wire up cross-references
        channel_hub.set_session_hub(Arc::clone(&session_hub));
        session_hub.set_channel_hub(Arc::clone(&channel_hub));
        session_hub.set_agent_hub(Arc::clone(&agent_hub));
        agent_hub.set_session_hub(Arc::clone(&session_hub));

        // 2. Channel plugins — start plugins for each channel in settings.json
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

            if let Some(abort_handle) =
                channel_hub.start_plugin(plugin_dir, &name).await
            {
                services.register_channel(&name, abort_handle);
            }
        }

        // 3. Subscribe to hub events → sync to ServiceStatusManager → Dashboard
        let hub_services = Arc::clone(services);
        let mut agent_hub_rx = agent_hub.subscribe();
        tokio::spawn(async move {
            while let Ok(event) = agent_hub_rx.recv().await {
                match event {
                    HubEvent::OnAgentSpawned { key, kind } => {
                        eprintln!("[daemon] agent spawned: {} ({})", key, kind);
                        hub_services.add_agent(key, kind);
                    }
                    HubEvent::OnAgentKilled { key } => {
                        eprintln!("[daemon] agent killed: {}", key);
                        hub_services.remove_agent(&key);
                    }
                    _ => {}
                }
            }
        });

        // 4. Web server (Axum)
        let web_services = Arc::clone(services);
        let web_handle = tokio::spawn(async move {
            run_web_server(
                common::config::DEFAULT_PORT,
                dist_path,
                web_services,
            )
            .await
        });

        // 5. Tunnel
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
