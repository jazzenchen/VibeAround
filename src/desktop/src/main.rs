// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agent_detection;
mod desktop_detection;
mod onboarding;
mod profiles;
mod startkit;
mod tray;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::{AppHandle, Manager, Runtime};
use tokio::sync::{Mutex, Notify};

use onboarding::{OnboardingGate, OnboardingSessions};
use startkit::StartkitRunState;

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

/// Return the scoped credential for external agent-as-API clients.
#[tauri::command]
fn get_local_agent_api_token() -> Option<common::auth::AuthFile> {
    common::auth::read_local_agent_api_token_file()
}

#[tauri::command]
fn get_app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    }
}

#[tauri::command]
async fn rescan_agent_entries() -> Result<agent_detection::AgentDetectionFile, String> {
    agent_detection::scan_and_persist()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn rescan_desktop_app_entries() -> Result<desktop_detection::DesktopAppDetectionFile, String>
{
    desktop_detection::scan_and_persist()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_desktop_app_entries() -> Option<desktop_detection::DesktopAppDetectionFile> {
    desktop_detection::read_detected_desktop_apps()
}

#[tauri::command]
fn check_selected_launch_entry() -> Result<bool, String> {
    let cfg = common::config::ensure_loaded();
    let prefs = common::agent_state::read_prefs();
    let agent_id = common::agent_state::resolve_selected_agent(&prefs, &cfg);
    let agent = common::resources::agent_by_alias(&agent_id)
        .ok_or_else(|| format!("unknown agent: '{agent_id}'"))?;

    let configured_path = common::agent_state::resolve_agent_executable_path(&prefs, &agent.id);
    let exists = if agent.direct_only {
        configured_path.as_deref().is_some_and(Path::is_file)
            || desktop_detection::refresh_known_agent_and_persist(&agent.id)
                .map_err(|error| error.to_string())?
                .is_some()
            || selected_desktop_app_cached(&agent.id, configured_path.as_deref())
    } else {
        configured_path.as_deref().is_some_and(Path::is_file)
            || common::agent_detection::selected_candidate(&agent.id)
                .as_ref()
                .is_some_and(|candidate| Path::new(&candidate.path).is_file())
    };
    Ok(exists)
}

fn selected_desktop_app_cached(agent_id: &str, configured_path: Option<&Path>) -> bool {
    let Some(detected) = desktop_detection::read_detected_desktop_apps() else {
        return false;
    };
    let Some(entry) = detected
        .apps
        .get(agent_id)
        .and_then(|detection| detection.entry.as_ref())
    else {
        return false;
    };
    if entry.source == "windows_start_apps" {
        return configured_path
            .map(|path| path.to_string_lossy().eq_ignore_ascii_case(&entry.path))
            .unwrap_or(true);
    }
    Path::new(&entry.path).is_file()
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

    let port = common::config::DEFAULT_PORT;
    let daemon = Arc::new(server::ServerDaemon::new(port));
    let tunnels = daemon.tunnels();
    #[cfg(windows)]
    let graceful_exit_started = Arc::new(std::sync::atomic::AtomicBool::new(false));

    // Persist both daemon-lifetime tokens before the desktop-ui starts
    // rendering or a bridge-backed agent can be launched.
    if let Err(e) = daemon.persist_auth_tokens() {
        tracing::info!("[VibeAround] Failed to persist auth tokens: {}", e);
    }

    let onboarding_needed = onboarding::needs_onboarding();

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
        .manage(OnboardingSessions::default())
        .manage(OnboardingActive(std::sync::atomic::AtomicBool::new(
            onboarding_needed,
        )))
        .manage(StartkitRunState::default())
        .invoke_handler(tauri::generate_handler![
            get_auth_token,
            get_local_agent_api_token,
            get_app_info,
            rescan_agent_entries,
            rescan_desktop_app_entries,
            get_desktop_app_entries,
            check_selected_launch_entry,
            open_external_url,
            restart_services,
            set_ui_locale,
            onboarding::get_settings,
            onboarding::list_channel_plugins,
            onboarding::save_settings,
            onboarding::uninstall_agent_integrations,
            onboarding::install_plugin,
            onboarding::check_plugin_status,
            onboarding::plugin_auth_start,
            onboarding::plugin_auth_wait,
            onboarding::plugin_auth_cancel,
            onboarding::finish_onboarding,
            onboarding::list_agents,
            onboarding::scan_agent_install_status,
            onboarding::check_agent_updates,
            onboarding::check_plugin_updates,
            onboarding::scan_tunnel_status,
            onboarding::list_tunnels,
            onboarding::list_plugin_registry,
            onboarding::list_managed_plugins,
            onboarding::refresh_managed_plugins,
            onboarding::install_managed_plugin,
            onboarding::test_web_search,
            startkit::startkit_manifest,
            startkit::startkit_plan,
            startkit::startkit_scan,
            startkit::start_startkit_install,
            startkit::cancel_startkit_install,
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
            profiles::profiles_google_oauth_status,
            profiles::profiles_google_oauth_login,
            profiles::profiles_test_connection,
            profiles::launcher_list_sessions,
            profiles::launcher_list_workspaces,
            profiles::launcher_get_preferences,
            profiles::launcher_agent_executable_resolution,
            profiles::launcher_agent_executable_latest,
            profiles::launcher_update_agent,
            profiles::launcher_set_default,
            profiles::launcher_set_agent_profile,
            profiles::launcher_set_agent_launch_args,
            profiles::launcher_set_agent_executable_path,
            profiles::launcher_set_selected_agent,
            profiles::launcher_set_terminal,
            profiles::launcher_set_workspace,
            profiles::launcher_remove_workspace,
            profiles::launcher_reorder_workspaces,
            profiles::launcher_set_compatibility_bridge,
            profiles::launcher_set_local_agent_api_enabled,
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
        .run({
            #[cfg(windows)]
            let graceful_exit_started = Arc::clone(&graceful_exit_started);
            move |_app, event| {
                #[cfg(windows)]
                if let tauri::RunEvent::ExitRequested { api, code, .. } = &event {
                    if *code != Some(tauri::RESTART_EXIT_CODE)
                        && !graceful_exit_started.swap(true, std::sync::atomic::Ordering::SeqCst)
                    {
                        api.prevent_exit();
                        let app_handle = _app.clone();
                        let exit_code = code.unwrap_or(0);
                        tauri::async_runtime::spawn(async move {
                            if let Err(error) = stop_daemon(&app_handle).await {
                                tracing::warn!(
                                    "[VibeAround] failed to stop daemon before exit: {}",
                                    error
                                );
                            }
                            app_handle.exit(exit_code);
                        });
                        return;
                    }
                }
                // Final safety net for child processes if graceful daemon
                // shutdown was skipped or interrupted.
                if let tauri::RunEvent::Exit = event {
                    common::process::registry::ChildRegistry::global().kill_all();
                    common::previews::shutdown_kill_all_ports();
                }
            }
        });
}
