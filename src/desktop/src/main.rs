// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod tray;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;

use common::config;
use common::tunnels::{self, TunnelProvider, TUNNEL_PASSWORD_URL};
use server::{run_telegram_bot, run_web_server};

/// Shared tunnel URL (set when tunnel is ready). Used by tray to open/copy public URL.
pub struct TunnelState(pub Arc<RwLock<Option<String>>>);

const WEB_DASHBOARD_PORT: u16 = 5182;

fn main() {
    let config = config::ensure_loaded();
    let tunnel_url: Arc<RwLock<Option<String>>> = Arc::new(RwLock::new(None));
    let tunnel_provider = config.tunnel_provider;

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(TunnelState(Arc::clone(&tunnel_url)))
        .setup(move |_app| {
            tray::setup(_app)?;
            // Run web server (SPA + WebSocket + session API) on Tauri's async runtime.
            // Feishu webhook needs the server; start Feishu bot first to get state, then run server.
            let dist_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../web/dist");
            tauri::async_runtime::spawn(async move {
                let feishu_state = common::im::channels::feishu::run_feishu_bot().await;
                if let Err(e) = run_web_server(WEB_DASHBOARD_PORT, dist_path, feishu_state).await {
                    eprintln!("[VibeAround] Web server error: {}", e);
                }
            });
            // Start tunnel (provider from settings.json); once URL is ready, store it and keep process alive.
            tauri::async_runtime::spawn({
                let tunnel_url = Arc::clone(&tunnel_url);
                let provider = tunnel_provider;
                async move {
                    eprintln!("[VibeAround] Tunnel ({})", provider.as_str());
                    let config = config::ensure_loaded();
                    match tunnels::start_web_tunnel_with_provider(provider, config).await {
                        Ok((guard, url)) => {
                            if let Ok(mut w) = tunnel_url.write() {
                                *w = Some(url.clone());
                            }
                            eprintln!("[VibeAround] Tunnel URL: {}", url);
                            if matches!(provider, TunnelProvider::Localtunnel) {
                                let _ = tunnels::ping_tunnel_with_bypass(&url).await;
                                if let Ok(Some(pw)) = tunnels::fetch_tunnel_password().await {
                                    eprintln!(
                                        "[VibeAround] Tunnel password (for loca.lt page): {} — or visit {}",
                                        pw,
                                        TUNNEL_PASSWORD_URL
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
                }
            });
            // Telegram bot (long polling); no-op if TELEGRAM_BOT_TOKEN not set
            tauri::async_runtime::spawn(run_telegram_bot());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![ping])
        .run(tauri::generate_context!())
        .expect("error while running VibeAround");
}

/// Ping command for Tray IPC (used by tray Mini SPA).
#[tauri::command]
fn ping() -> String {
    "pong".into()
}
