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

    // Early check: if the port is already in use, another instance is likely running.
    // Print a warning before Tauri's single_instance plugin silently exits the new process.
    let port = common::config::DEFAULT_PORT;
    if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
        eprintln!(
            "[VibeAround] ⚠️  Another instance is already running (port {} in use). \
             This instance will exit.",
            port
        );
    }

    let daemon = server::ServerDaemon::new(port);
    let services = daemon.services();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            eprintln!("[VibeAround] ⚠️  Another instance tried to start, focusing existing window");
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .manage(AppServiceManager(services))
        .setup(move |app| {
            tray::setup(app)?;

            // Show the main window on startup
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }

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
