// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod tray;

use std::path::PathBuf;
use std::sync::Arc;

use common::config;
use tauri::Manager;

/// Shared ServiceManager, injected into Tauri state for tray and IPC access.
pub struct AppServiceManager(pub Arc<common::service::ServiceManager>);

fn main() {
    let _config = config::ensure_loaded();

    let daemon = server::ServerDaemon::new(common::config::DEFAULT_PORT);
    let services = daemon.services();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            eprintln!("[VibeAround] Another instance detected, focusing existing window");
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .manage(AppServiceManager(services))
        .setup(move |app| {
            tray::setup(app)?;

            // Start the full ServerDaemon (web server + IM bots + tunnel)
            let dist_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../web/dist");
            tauri::async_runtime::spawn(async move {
                if let Err(e) = daemon.start(dist_path).await {
                    eprintln!("[VibeAround] Daemon error: {}", e);
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running VibeAround");
}
