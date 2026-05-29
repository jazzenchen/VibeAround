// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod onboarding;
mod profiles;
mod tray;

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Manager, Runtime};
use tokio::sync::{Mutex, Notify};

use onboarding::{OnboardingGate, OnboardingInstallState, OnboardingSessions};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    version: &'static str,
    os: &'static str,
    arch: &'static str,
}

/// Shared `TunnelManager` handle, injected into Tauri state for the
/// tray (live tunnel menu item) and any IPC command that needs the
/// current tunnel URL.
pub struct AppTunnels(pub Arc<common::tunnels::TunnelManager>);

/// Whether the app is currently in onboarding mode (tray reads this).
pub struct OnboardingActive(pub std::sync::atomic::AtomicBool);

pub struct DaemonController {
    daemon: Arc<server::ServerDaemon>,
    dist_path: PathBuf,
    running: Mutex<Option<server::RunningDaemon>>,
}

impl DaemonController {
    pub fn new(daemon: Arc<server::ServerDaemon>, dist_path: PathBuf) -> Self {
        Self {
            daemon,
            dist_path,
            running: Mutex::new(None),
        }
    }

    pub async fn start(&self) -> Result<(), String> {
        let mut running = self.running.lock().await;
        if running.is_some() {
            return Ok(());
        }

        let daemon = self
            .daemon
            .start_background(self.dist_path.clone())
            .await
            .map_err(|e| e.to_string())?;
        *running = Some(daemon);
        Ok(())
    }

    pub async fn stop(&self) {
        let mut running = self.running.lock().await;
        if let Some(daemon) = running.take() {
            daemon.stop().await;
        }
    }

    pub async fn restart(&self) -> Result<(), String> {
        self.stop().await;
        self.start().await
    }
}

pub async fn start_daemon<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let controller = app
        .try_state::<DaemonController>()
        .ok_or_else(|| "daemon controller state is missing".to_string())?;
    controller.start().await
}

pub async fn stop_daemon<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let controller = app
        .try_state::<DaemonController>()
        .ok_or_else(|| "daemon controller state is missing".to_string())?;
    controller.stop().await;
    Ok(())
}

pub async fn restart_daemon<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let controller = app
        .try_state::<DaemonController>()
        .ok_or_else(|| "daemon controller state is missing".to_string())?;
    controller.restart().await
}

/// Return the current web-server auth token + port so the desktop-ui (a
/// cross-origin Tauri webview) can authenticate against the daemon.
///
/// Reads directly from `~/.vibearound/auth.json`, which `ServerDaemon` writes
/// on every start. Returns `None` before the daemon has started for the
/// first time.
#[tauri::command]
fn get_auth_token() -> Option<common::auth::AuthFile> {
    common::auth::read_token_file()
}

#[tauri::command]
fn get_app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    }
}

/// Open an HTTP URL in the user's default external browser.
///
/// We can't use `window.open` from the desktop-ui because it creates a
/// Tauri child webview instead of hitting the OS-level handler. This
/// command shells out via the `open` crate, which is what the tray also
/// uses for "Open Local Dashboard".
///
/// Used for dashboard/tunnel links and trusted app-owned external links such
/// as GitHub release downloads.
#[tauri::command]
fn open_external_url(url: String) -> Result<(), String> {
    // Minimal guard: only allow http/https schemes. Prevents a rogue
    // caller from asking us to execute `file://` or `javascript:` URIs.
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(format!("refused to open non-http URL: {url}"));
    }
    open::that(&url)
        .map(|_| ())
        .map_err(|e| format!("failed to open url: {e}"))
}

#[tauri::command]
async fn restart_services<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    restart_daemon(&app).await
}

#[tauri::command]
fn set_ui_locale<R: Runtime>(app: AppHandle<R>, locale: String) -> Result<(), String> {
    tray::set_ui_locale(&app, &locale)
}

fn main() {
    common::logging::init();

    // Fast-path: if our port is already in use, exit immediately before
    // allocating Tauri resources. tauri_plugin_single_instance (below) is the
    // real guard, but this avoids a full Tauri init just to discover the duplicate.
    let port = common::config::DEFAULT_PORT;
    if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
        tracing::info!(
            "[VibeAround] Another instance is already running (port {} in use). Exiting.",
            port
        );
        std::process::exit(0);
    }

    let daemon = Arc::new(server::ServerDaemon::new(port));
    let tunnels = daemon.tunnels();

    // Persist the auth token immediately so the desktop-ui (which runs in
    // a Tauri webview that starts rendering before the daemon has fully
    // booted) can read `~/.vibearound/auth.json` from its first render.
    if let Err(e) = daemon.persist_auth_token() {
        tracing::info!("[VibeAround] Failed to persist auth token: {}", e);
    }

    let onboarding_needed = onboarding::needs_onboarding();

    // Rewrite each enabled coding agent's MCP config with the fresh
    // token-bearing URL *before* the HTTP listener binds. This closes a
    // race window where auth.json already carries the new token but the
    // MCP config files on disk still reference the previous run's token:
    // a coding agent that happens to boot during that window would cache
    // the stale URL and 401 on every tool call until restarted.
    //
    // Skipped on first run (onboarding_needed) because no agents are
    // enabled yet — the onboarding install flow calls sync_integrations
    // itself with the freshly populated settings. Skipping here also
    // avoids a pointless uninstall sweep over files that don't exist yet.
    if !onboarding_needed {
        common::agent::sync_integrations(&onboarding::get_settings_value());
    }
    let gate = Arc::new(Notify::new());

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tracing::info!(
                "[VibeAround] ⚠️  Another instance tried to start, focusing existing window"
            );
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .manage(AppTunnels(tunnels))
        .manage(OnboardingGate {
            notify: Arc::clone(&gate),
        })
        .manage(OnboardingSessions {
            plugin_sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),
        })
        .manage(OnboardingActive(std::sync::atomic::AtomicBool::new(
            onboarding_needed,
        )))
        .manage(OnboardingInstallState::default())
        .invoke_handler(tauri::generate_handler![
            get_auth_token,
            get_app_info,
            open_external_url,
            restart_services,
            set_ui_locale,
            onboarding::get_settings,
            onboarding::list_channel_plugins,
            onboarding::save_settings,
            onboarding::install_plugin,
            onboarding::check_plugin_status,
            onboarding::plugin_auth_start,
            onboarding::plugin_auth_wait,
            onboarding::plugin_auth_cancel,
            onboarding::finish_onboarding,
            onboarding::list_agents,
            onboarding::list_tunnels,
            onboarding::list_plugin_registry,
            onboarding::get_install_manifest,
            onboarding::start_onboarding_install,
            onboarding::cancel_onboarding_install,
            profiles::profiles_list,
            profiles::profiles_get,
            profiles::profiles_create,
            profiles::profiles_upsert,
            profiles::profiles_delete,
            profiles::profiles_reorder,
            profiles::profiles_launch,
            profiles::profiles_launch_resume,
            profiles::profiles_launch_default,
            profiles::profiles_launch_direct,
            profiles::profiles_launch_direct_resume,
            profiles::profiles_catalog,
            profiles::launcher_list_sessions,
            profiles::launcher_list_workspaces,
            profiles::launcher_get_preferences,
            profiles::launcher_set_default,
            profiles::launcher_set_agent_profile,
            profiles::launcher_set_selected_agent,
            profiles::launcher_set_terminal,
            profiles::launcher_set_workspace,
            profiles::launcher_remove_workspace,
            profiles::launcher_reorder_workspaces,
            profiles::launcher_set_compatibility_bridge,
            profiles::launcher_set_profile_connection,
        ])
        .setup({
            let daemon = Arc::clone(&daemon);
            move |app| {
                // Resolve the web dashboard `dist/` directory.
                //
                // - **Dev** (`cargo tauri dev`): read from the source tree so hot
                //   edits to `src/web/dist` are picked up without rebundling.
                // - **Release**: read from the Tauri bundle's resource dir. The
                //   resources glob `../web/dist/**/*` in `tauri.conf.json` copies
                //   files under `<resource_dir>/_up_/web/dist/`.
                //
                // Using `env!("CARGO_MANIFEST_DIR")` unconditionally would bake
                // the *build machine's* absolute source path into the release
                // binary — on every other machine that path doesn't exist, the
                // daemon fails to locate the dashboard, and users hit a broken
                // install.
                let dist_path: PathBuf = {
                    #[cfg(debug_assertions)]
                    {
                        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../web/dist")
                    }
                    #[cfg(not(debug_assertions))]
                    {
                        app.path()
                            .resource_dir()
                            .map_err(|e| format!("failed to resolve resource_dir: {e}"))?
                            .join("_up_")
                            .join("web")
                            .join("dist")
                    }
                };
                app.manage(DaemonController::new(Arc::clone(&daemon), dist_path));

                tray::setup(app)?;

                // Show the window immediately — the splash screen in index.html
                // is visible while React loads and the daemon starts.
                if let Some(w) = app.get_webview_window("main") {
                    if onboarding_needed {
                        let _ = w.eval("window.location.replace('/onboarding')");
                    }
                    let _ = w.show();
                    let _ = w.set_focus();
                }

                // Start the daemon — immediately if no onboarding needed,
                // otherwise wait for the onboarding gate signal.
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    if onboarding_needed {
                        tracing::info!("[VibeAround] Waiting for onboarding to complete…");
                        gate.notified().await;
                        tracing::info!("[VibeAround] Onboarding complete, starting daemon…");

                        // Mark onboarding as done for tray
                        if let Some(state) = app_handle.try_state::<OnboardingActive>() {
                            state.0.store(false, std::sync::atomic::Ordering::Relaxed);
                        }
                    }

                    if let Err(e) = start_daemon(&app_handle).await {
                        tracing::info!("[VibeAround] Daemon error: {}", e);
                    }

                    // Onboarding path: settings were empty when we ran the
                    // early sync before Tauri started, so run it now that
                    // the user has picked their enabled agents. The
                    // steady-state path already ran this pre-binding, so
                    // re-running would be wasted work.
                    if onboarding_needed {
                        common::agent::sync_integrations(&onboarding::get_settings_value());
                    }
                });

                Ok(())
            }
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building VibeAround")
        .run(|_app, event| {
            // On app exit — whether via Cmd-Q, dock quit, window close, or
            // tray Quit — synchronously SIGKILL every registered child
            // process. This is the last line of defense against orphaned
            // plugin/agent processes; the graceful stop paths in
            // RunningDaemon::stop also run but may be skipped entirely on
            // abrupt exit (e.g. signal-driven shutdown before the async
            // runtime has been able to drain its tasks).
            if let tauri::RunEvent::Exit = event {
                common::process::registry::ChildRegistry::global().kill_all();
                common::previews::shutdown_kill_all_ports();
            }
        });
}
