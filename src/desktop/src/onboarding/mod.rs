//! Onboarding: first-run setup wizard.
//! Checks whether settings.json has `"onboarded": true`; exposes Tauri IPC
//! commands so the desktop-ui frontend can read/write settings and signal completion.

mod agent_integrations;
mod install_orchestration;
pub(crate) mod plugin_install;
mod plugin_session;

pub use plugin_install::{
    __cmd__check_plugin_status,
    // Re-export Tauri macro-generated handler identifiers so generate_handler! works
    // when commands are referenced as `onboarding::install_plugin`.
    __cmd__install_plugin,
    __tauri_command_name_check_plugin_status,
    __tauri_command_name_install_plugin,
    check_plugin_status,
    install_plugin,
};
pub use plugin_session::PluginSession;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tokio::sync::{Mutex, Notify};

use crate::{restart_daemon, OnboardingActive};
use common::{config, plugins};

use crate::startkit::{StartkitChoices, StartkitItemReport, StartkitItemStatus};

// ---------------------------------------------------------------------------
// Shared state types
// ---------------------------------------------------------------------------

pub struct OnboardingGate {
    pub notify: Arc<Notify>,
}

pub struct OnboardingSessions {
    pub plugin_sessions: Arc<Mutex<HashMap<String, PluginSession>>>,
}

/// State for the granular onboarding install flow.
pub struct OnboardingInstallState {
    pub cancelled: Arc<AtomicBool>,
    pub child_process: Arc<Mutex<Option<tokio::process::Child>>>,
    pub log_file: Arc<Mutex<Option<std::fs::File>>>,
}

impl Default for OnboardingInstallState {
    fn default() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            child_process: Arc::new(Mutex::new(None)),
            log_file: Arc::new(Mutex::new(None)),
        }
    }
}

/// Progress event emitted to the frontend during onboarding install.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallProgressEvent {
    pub id: String,
    pub label: String,
    pub status: String,
    pub message: Option<String>,
}

/// Task info returned by `get_install_manifest`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallTaskInfo {
    pub id: String,
    pub label: String,
}

// ---------------------------------------------------------------------------
// Settings helpers
// ---------------------------------------------------------------------------

fn settings_path() -> PathBuf {
    config::data_dir().join("settings.json")
}

fn read_settings_value() -> Value {
    let path = settings_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

fn write_settings_value(val: &Value) -> Result<(), String> {
    // settings.json holds bot tokens, webhook secrets, and tunnel credentials
    // in plain text (by design — the user edits this file directly). Ensure
    // other local users cannot read it. No-op on Windows.
    config::write_settings_json(val)
}

// ---------------------------------------------------------------------------
// Onboarding gate
// ---------------------------------------------------------------------------

/// Read current settings (exposed for startup integration sync).
#[allow(dead_code)]
pub fn get_settings_value() -> serde_json::Value {
    read_settings_value()
}

pub fn needs_onboarding() -> bool {
    let val = read_settings_value();
    !val.get("onboarded")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Resource summary types — expose agent/tunnel/plugin definitions to frontend
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
pub struct AgentSummary {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub install_type: Option<String>,
    pub pty_command: String,
    pub acp_program: String,
    pub acp_args: Vec<String>,
    pub acp_npm_package: Option<String>,
    pub acp_bin_name: Option<String>,
}

#[derive(serde::Serialize)]
pub struct TunnelSummary {
    pub id: String,
    pub display_name: String,
}

#[derive(serde::Serialize)]
pub struct PluginSummary {
    pub id: String,
    pub kind: String,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub github: String,
}

// ---------------------------------------------------------------------------
// Tauri commands — settings
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_settings() -> Result<Value, String> {
    Ok(read_settings_value())
}

#[tauri::command]
pub fn list_channel_plugins() -> Result<Vec<plugins::DiscoveredPluginSummary>, String> {
    Ok(plugins::channel::list_summaries())
}

#[tauri::command]
pub fn save_settings<R: Runtime>(app: AppHandle<R>, settings: Value) -> Result<(), String> {
    write_settings_value(&settings)?;
    let _ = app.emit(crate::tray::LAUNCH_CONFIG_CHANGED_EVENT, ());
    Ok(())
}

#[tauri::command]
pub async fn uninstall_agent_integrations(
    remove_mcp: bool,
    remove_skills: bool,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        common::agent::uninstall_legacy_integrations(remove_mcp, remove_skills)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Tauri commands — resource queries
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_agents() -> Vec<AgentSummary> {
    common::resources::AGENTS
        .iter()
        .map(|a| AgentSummary {
            id: a.id.clone(),
            display_name: a.display_name.clone(),
            description: a.description.clone(),
            install_type: a.install.as_ref().map(|i| i.install_type.clone()),
            pty_command: a.pty.command.clone(),
            acp_program: a.acp.program.clone(),
            acp_args: a.acp.args.clone(),
            acp_npm_package: a.acp.npm_package.clone(),
            acp_bin_name: a.acp.bin_name.clone(),
        })
        .collect()
}

#[tauri::command]
pub async fn scan_agent_install_status(
    settings: Value,
    choices: StartkitChoices,
) -> Result<Vec<StartkitItemReport>, String> {
    let all_agent_ids = common::resources::AGENTS
        .iter()
        .map(|agent| agent.id.clone())
        .collect::<Vec<_>>();
    let scan_choices = StartkitChoices {
        agents: all_agent_ids,
        tunnel: "none".to_string(),
        channels: Vec::new(),
        source: choices.source,
        toolchain_mode: choices.toolchain_mode,
        shell_path: false,
    };
    let startkit_report = crate::startkit::scan(&settings, &scan_choices, None)
        .await
        .map_err(|error| error.to_string())?;
    let mut startkit_reports = startkit_report
        .reports
        .into_iter()
        .map(|report| (report.id.clone(), report))
        .collect::<HashMap<_, _>>();

    Ok(common::resources::AGENTS
        .iter()
        .map(|agent| {
            let report_id = format!("agents.{}.cli", agent.id);
            startkit_reports
                .remove(&report_id)
                .unwrap_or_else(|| path_agent_report(agent, report_id))
        })
        .collect())
}

fn path_agent_report(agent: &common::resources::AgentDef, report_id: String) -> StartkitItemReport {
    let program =
        program_from_command(&agent.pty.command).unwrap_or_else(|| agent.acp.program.clone());
    let path = resolve_program_path(&program);
    let installed = path.is_some();
    StartkitItemReport {
        id: report_id,
        label: agent.display_name.clone(),
        group: "agents".to_string(),
        category: "agents".to_string(),
        status: if installed {
            StartkitItemStatus::Ok
        } else {
            StartkitItemStatus::Missing
        },
        severity: None,
        version: None,
        path,
        message: Some(if installed {
            format!("{program} found")
        } else {
            format!("{program} not found in PATH")
        }),
        actions: Vec::new(),
        secret: false,
        settings_key: None,
    }
}

fn program_from_command(command: &str) -> Option<String> {
    command
        .split_whitespace()
        .next()
        .map(|program| program.trim_matches(['"', '\'']).to_string())
        .filter(|program| !program.is_empty())
}

fn resolve_program_path(program: &str) -> Option<String> {
    let lookup = if cfg!(windows) { "where" } else { "which" };
    let output = common::process::env::std_command(lookup)
        .arg(program)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

#[tauri::command]
pub fn list_tunnels() -> Vec<TunnelSummary> {
    common::resources::TUNNELS
        .iter()
        .map(|t| TunnelSummary {
            id: t.id.clone(),
            display_name: t.display_name.clone(),
        })
        .collect()
}

#[tauri::command]
pub fn list_plugin_registry() -> Vec<PluginSummary> {
    common::resources::PLUGINS
        .iter()
        .filter(|p| p.is_kind("channel"))
        .map(|p| PluginSummary {
            id: p.id.clone(),
            kind: p.kind.clone(),
            slug: p.install_dir_name().to_string(),
            name: p.name.clone(),
            description: p.description.clone(),
            github: p.github.clone(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tauri commands — onboarding flow
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginAuthStartRequest {
    pub plugin_id: String,
    pub config: Value,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginAuthWaitRequest {
    pub plugin_id: String,
    pub params: Value,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginAuthCancelRequest {
    pub plugin_id: String,
}

#[tauri::command]
pub async fn plugin_auth_start(
    state: State<'_, OnboardingSessions>,
    request: PluginAuthStartRequest,
) -> Result<Value, String> {
    let mut sessions = state.plugin_sessions.lock().await;
    if let Some(mut existing) = sessions.remove(&request.plugin_id) {
        plugin_session::shutdown_plugin_session(&mut existing).await;
    }

    let mut session =
        plugin_session::spawn_auth_session(&request.plugin_id, request.config.clone())
            .await
            .map_err(|e| e.to_string())?;

    let result: Value =
        plugin_session::plugin_request(&mut session, "login_qr_start", request.config)
            .await
            .map_err(|e| e.to_string())?;

    sessions.insert(request.plugin_id, session);
    Ok(result)
}

#[tauri::command]
pub async fn plugin_auth_wait(
    state: State<'_, OnboardingSessions>,
    request: PluginAuthWaitRequest,
) -> Result<Value, String> {
    let mut sessions = state.plugin_sessions.lock().await;
    let session = sessions
        .get_mut(&request.plugin_id)
        .ok_or_else(|| format!("auth session for '{}' not started", request.plugin_id))?;

    let result: Value = plugin_session::plugin_request(session, "login_qr_wait", request.params)
        .await
        .map_err(|e| e.to_string())?;

    // Shutdown on success
    if result
        .get("connected")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        if let Some(mut session) = sessions.remove(&request.plugin_id) {
            plugin_session::shutdown_plugin_session(&mut session).await;
        }
    }

    Ok(result)
}

#[tauri::command]
pub async fn plugin_auth_cancel(
    state: State<'_, OnboardingSessions>,
    request: PluginAuthCancelRequest,
) -> Result<(), String> {
    let mut sessions = state.plugin_sessions.lock().await;
    if let Some(mut session) = sessions.remove(&request.plugin_id) {
        plugin_session::shutdown_plugin_session(&mut session).await;
    }
    Ok(())
}

/// Called after `start_onboarding_install` completes. Signals the daemon gate
/// and navigates the user to the dashboard.
#[tauri::command]
pub async fn finish_onboarding<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, OnboardingSessions>,
) -> Result<(), String> {
    // Clean up any remaining auth sessions
    let mut sessions = state.plugin_sessions.lock().await;
    for (_, mut session) in sessions.drain() {
        plugin_session::shutdown_plugin_session(&mut session).await;
    }
    drop(sessions);

    let mut settings = read_settings_value();
    if let Some(obj) = settings.as_object_mut() {
        obj.insert("onboarded".into(), serde_json::json!(true));
    }
    write_settings_value(&settings)?;

    let _ = app.emit("onboarding-complete", ());

    if let Some(active) = app.try_state::<OnboardingActive>() {
        let was_onboarding = active.0.swap(false, Ordering::Relaxed);
        if was_onboarding {
            if let Some(gate) = app.try_state::<OnboardingGate>() {
                gate.notify.notify_one();
            }
        } else {
            restart_daemon(&app).await?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tauri commands — install manifest + orchestrated install
// ---------------------------------------------------------------------------

/// Returns the list of install tasks for the given settings, so the frontend
/// can pre-populate the progress list before install starts.
#[tauri::command]
pub fn get_install_manifest(
    settings: Value,
    scope: Option<install_orchestration::InstallScope>,
) -> Vec<InstallTaskInfo> {
    install_orchestration::get_install_manifest(&settings, scope.unwrap_or_default())
}

/// Orchestrates the full onboarding install sequence. Saves settings, then
/// installs MCP configs, skills, ACP agents, and channel plugins one by one,
/// emitting `"onboarding-install-progress"` events for each task.
#[tauri::command]
pub async fn start_onboarding_install<R: Runtime>(
    app: AppHandle<R>,
    install_state: State<'_, OnboardingInstallState>,
    settings: Value,
    scope: Option<install_orchestration::InstallScope>,
) -> Result<(), String> {
    install_orchestration::start(app, &install_state, settings, scope.unwrap_or_default()).await
}

/// Cancel a running onboarding install.
#[tauri::command]
pub async fn cancel_onboarding_install(
    install_state: State<'_, OnboardingInstallState>,
) -> Result<(), String> {
    install_orchestration::cancel(&install_state).await
}
