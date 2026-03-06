//! Standalone VibeAround server binary — full-featured: web dashboard, IM bots, tunnel.
//! All configuration comes from settings.json. Equivalent to the Tauri desktop app without the native window.

use std::path::PathBuf;

use common::config;
use common::tunnels::{self, TunnelProvider, TUNNEL_PASSWORD_URL};

const DEFAULT_PORT: u16 = 5182;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = config::ensure_loaded();
    let tunnel_provider = config.tunnel_provider;

    let dist_path = PathBuf::from("web").join("dist");

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        // 1. Start Feishu bot (returns webhook state for the web server routes)
        let feishu_state = common::im::channels::feishu::run_feishu_bot().await;
        if feishu_state.is_some() {
            eprintln!("[VibeAround] Feishu bot started");
        }

        // 2. Start web server (HTTP + WS + Feishu webhook)
        eprintln!("[VibeAround] Starting web server on port {}", DEFAULT_PORT);
        let web = tokio::spawn(server::run_web_server(DEFAULT_PORT, dist_path, feishu_state));

        // 3. Start Telegram bot (no-op if bot_token not configured)
        if config.telegram_bot_token.is_some() {
            eprintln!("[VibeAround] Starting Telegram bot");
        }
        tokio::spawn(server::run_telegram_bot());

        // 4. Start tunnel
        tokio::spawn(async move {
            eprintln!("[VibeAround] Tunnel ({})", tunnel_provider.as_str());
            let config = config::ensure_loaded();
            match tunnels::start_web_tunnel_with_provider(tunnel_provider, config).await {
                Ok((guard, url)) => {
                    eprintln!("[VibeAround] Tunnel URL: {}", url);
                    if matches!(tunnel_provider, TunnelProvider::Localtunnel) {
                        let _ = tunnels::ping_tunnel_with_bypass(&url).await;
                        if let Ok(Some(pw)) = tunnels::fetch_tunnel_password().await {
                            eprintln!(
                                "[VibeAround] Tunnel password (for loca.lt page): {} — or visit {}",
                                pw, TUNNEL_PASSWORD_URL
                            );
                        } else {
                            eprintln!(
                                "[VibeAround] To get tunnel password, visit: {}",
                                TUNNEL_PASSWORD_URL
                            );
                        }
                    }
                    guard.wait().await;
                }
                Err(e) => eprintln!("[VibeAround] Tunnel failed: {}", e),
            }
        });

        // Wait for either: web server exits, or Ctrl+C
        tokio::select! {
            result = web => {
                match result {
                    Ok(Ok(())) => eprintln!("[VibeAround] Web server stopped"),
                    Ok(Err(e)) => eprintln!("[VibeAround] Web server error: {}", e),
                    Err(e) => eprintln!("[VibeAround] Web server task panic: {}", e),
                }
            }
            _ = tokio::signal::ctrl_c() => {
                eprintln!("\n[VibeAround] Shutting down...");
            }
        }
    });

    // Runtime drop kills all spawned tasks + child processes
    std::process::exit(0);
}
