// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod onboarding;
mod tray;

use std::path::PathBuf;
use std::sync::Arc;

use common::config;
use tauri::Manager;
use tokio::sync::Notify;

use onboarding::OnboardingGate;

/// Shared ServiceManager, injected into Tauri state for tray and IPC access.
pub struct AppServiceManager(pub Arc<common::service::ServiceManager>);

/// Whether the app is currently in onboarding mode (tray reads this).
pub struct OnboardingActive(pub std::sync::atomic::AtomicBool);

fn main() {
    let _config = config::ensure_loaded();

    // Early check: if the port is already in use, another instance is likely running.
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

    let onboarding_needed = onboarding::needs_onboarding();
    let gate = Arc::new(Notify::new());

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
        .manage(OnboardingGate { notify: Arc::clone(&gate) })
        .manage(OnboardingActive(std::sync::atomic::AtomicBool::new(onboarding_needed)))
        .invoke_handler(tauri::generate_handler![
            onboarding::get_settings,
            onboarding::save_settings,
            onboarding::finish_onboarding,
        ])
        .setup(move |app| {
            tray::setup(app)?;

            // Show the main window on startup
            if let Some(w) = app.get_webview_window("main") {
                if onboarding_needed {
                    // Navigate to onboarding page
                    let _ = w.eval("window.location.replace('/onboarding')");
                }
                let _ = w.show();
                let _ = w.set_focus();
            }

            // Start the daemon — immediately if no onboarding needed,
            // otherwise wait for the onboarding gate signal.
            let dist_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../web/dist");
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if onboarding_needed {
                    eprintln!("[VibeAround] Waiting for onboarding to complete…");
                    gate.notified().await;
                    eprintln!("[VibeAround] Onboarding complete, starting daemon…");

                    // Reload config after onboarding wrote new settings
                    // (config is a OnceLock singleton, so we just proceed —
                    //  the daemon will re-read settings.json via its own paths)

                    // Mark onboarding as done for tray
                    if let Some(state) = app_handle.try_state::<OnboardingActive>() {
                        state.0.store(false, std::sync::atomic::Ordering::Relaxed);
                    }
                }

                if let Err(e) = daemon.start(dist_path).await {
                    eprintln!("[VibeAround] Daemon error: {}", e);
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running VibeAround");
}
