//! Onboarding: first-run setup wizard.
//! Checks whether settings.json has `"onboarded": true`; exposes Tauri IPC
//! commands so the desktop-ui frontend can read/write settings and signal completion.

pub(crate) mod plugin_install;
mod plugin_manager;
mod plugin_session;
mod search_settings;

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
pub use plugin_manager::{
    __cmd__install_managed_plugin, __cmd__list_managed_plugins, __cmd__refresh_managed_plugins,
    __tauri_command_name_install_managed_plugin, __tauri_command_name_list_managed_plugins,
    __tauri_command_name_refresh_managed_plugins, install_managed_plugin, list_managed_plugins,
    refresh_managed_plugins,
};
pub use search_settings::{
    __cmd__test_web_search, __tauri_command_name_test_web_search, test_web_search,
};

use std::collections::HashMap;
use std::process::{Output, Stdio};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tokio::io::AsyncReadExt;
use tokio::sync::{watch, Mutex, Notify};
use tokio::task::JoinSet;
use tokio::time::sleep;

use crate::{agent_detection, restart_daemon, OnboardingActive};
use common::{config, plugins};

use crate::startkit::{StartkitChoices, StartkitItemReport, StartkitItemStatus};

const AGENT_UPDATE_CHECK_TIMEOUT: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// Shared state types
// ---------------------------------------------------------------------------

pub struct OnboardingGate {
    pub notify: Arc<Notify>,
}

#[derive(Default)]
pub struct OnboardingSessions {
    plugin_sessions: Mutex<HashMap<String, PluginAuthSession>>,
}

#[derive(Clone)]
struct PluginAuthSession {
    method: plugin_session::PluginAuthMethod,
    session: Arc<Mutex<plugin_session::PluginSession>>,
    cancel: watch::Sender<bool>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUpdateCheckRequest {
    pub agent_ids: Vec<String>,
    pub choices: StartkitChoices,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginUpdateCheckRequest {
    pub plugin_ids: Vec<String>,
}

// ---------------------------------------------------------------------------
// Settings helpers
// ---------------------------------------------------------------------------

fn read_settings_value() -> Value {
    config::read_settings_json().unwrap_or_else(|_| serde_json::json!({}))
}

// ---------------------------------------------------------------------------
// Onboarding gate
// ---------------------------------------------------------------------------

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
    pub direct_only: bool,
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
pub async fn get_settings() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(read_settings_value)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn list_channel_plugins() -> Result<Vec<plugins::DiscoveredPluginSummary>, String> {
    tauri::async_runtime::spawn_blocking(plugins::channel::list_summaries)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn save_settings<R: Runtime>(
    app: AppHandle<R>,
    patch: Value,
) -> Result<config::SettingsSnapshot, String> {
    let snapshot =
        tauri::async_runtime::spawn_blocking(move || config::patch_settings_json(&patch))
            .await
            .map_err(|error| error.to_string())??;
    let _ = app.emit(crate::tray::LAUNCH_CONFIG_CHANGED_EVENT, ());
    Ok(snapshot)
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
        .filter(|a| a.supports_current_platform())
        .map(|a| AgentSummary {
            id: a.id.clone(),
            display_name: a.display_name.clone(),
            description: a.description.clone(),
            install_type: a.install.as_ref().map(|i| i.install_type.clone()),
            pty_command: a.pty_command_for_current_platform().to_string(),
            direct_only: a.direct_only,
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
    agent_cli_reports(&settings, &choices, &choices.agents)
        .await
        .map_err(|error| error.to_string())
}

async fn agent_cli_reports(
    settings: &Value,
    choices: &StartkitChoices,
    agent_ids: &[String],
) -> anyhow::Result<Vec<StartkitItemReport>> {
    let startkit_reports = crate::startkit::scan_agent_cli_reports(settings, choices, agent_ids)
        .await?
        .into_iter()
        .map(|report| (report.id.clone(), report))
        .collect::<HashMap<_, _>>();
    let mut reports = Vec::new();

    for agent_id in agent_ids {
        let report_id = format!("agents.{agent_id}.cli");
        if let Some(report) = startkit_reports.get(&report_id) {
            reports.push(report.clone());
            continue;
        }
        if let Some(agent) = common::resources::agent_by_id(agent_id) {
            reports.push(agent_install_report(agent.clone()).await);
        }
    }

    Ok(reports)
}

#[tauri::command]
pub async fn check_agent_updates(
    request: AgentUpdateCheckRequest,
) -> Result<Vec<StartkitItemReport>, String> {
    let mut tasks = JoinSet::new();

    for agent_id in request.agent_ids {
        let choices = request.choices.clone();
        tasks.spawn(async move { agent_update_report(agent_id, choices).await });
    }

    let mut reports = Vec::new();
    while let Some(result) = tasks.join_next().await {
        if let Some(report) = result.map_err(|error| error.to_string())? {
            reports.push(report);
        }
    }
    Ok(reports)
}

#[tauri::command]
pub async fn check_plugin_updates(
    request: PluginUpdateCheckRequest,
) -> Result<Vec<StartkitItemReport>, String> {
    let mut tasks = JoinSet::new();

    for plugin_id in request.plugin_ids {
        tasks.spawn(async move { plugin_update_report(plugin_id).await });
    }

    let mut reports = Vec::new();
    while let Some(result) = tasks.join_next().await {
        if let Some(report) = result.map_err(|error| error.to_string())? {
            reports.push(report);
        }
    }
    Ok(reports)
}

#[tauri::command]
pub async fn scan_tunnel_status(
    settings: Value,
    choices: StartkitChoices,
) -> Result<Vec<StartkitItemReport>, String> {
    crate::startkit::scan_tunnel_reports(&settings, &choices)
        .await
        .map_err(|error| error.to_string())
}

async fn agent_install_report(agent: common::resources::AgentDef) -> StartkitItemReport {
    let agent_id = agent.id.clone();
    let report_id = format!("agents.{}.cli", agent.id);

    let config = common::config::ensure_loaded();
    let candidate = common::agent_availability::resolve_agent_availability(
        &agent_id,
        common::agent_availability::AgentAvailabilityRequest {
            scan_policy: common::agent_availability::AgentScanPolicy::RefreshIfUnconfigured,
            toolchain_mode: config.toolchain_mode.as_str(),
            candidate_preference:
                common::agent_availability::AgentCandidatePreference::SystemToolchain,
            include_configured_version: true,
        },
    )
    .await
    .ok()
    .and_then(|availability| availability.selected);

    if let Some(candidate) = candidate {
        return StartkitItemReport {
            id: report_id,
            label: agent.display_name,
            group: "agents".to_string(),
            category: "agents".to_string(),
            status: StartkitItemStatus::Ok,
            severity: None,
            version: candidate.version,
            latest_version: None,
            path: Some(candidate.path),
            message: Some(format!(
                "{} selected from {}",
                agent.id, candidate.source_label
            )),
            actions: Vec::new(),
            manual_command: None,
            manual_url: None,
            secret: false,
            settings_key: None,
        };
    }

    let program = program_from_command(agent.pty_command_for_current_platform())
        .unwrap_or_else(|| agent.acp.program.clone());
    StartkitItemReport {
        id: report_id,
        label: agent.display_name.clone(),
        group: "agents".to_string(),
        category: "agents".to_string(),
        status: StartkitItemStatus::Blocked,
        severity: None,
        version: None,
        latest_version: None,
        path: None,
        message: Some(format!(
            "Install {program} on this computer, then scan again."
        )),
        actions: Vec::new(),
        manual_command: None,
        manual_url: None,
        secret: false,
        settings_key: None,
    }
}

async fn agent_update_report(
    agent_id: String,
    choices: StartkitChoices,
) -> Option<StartkitItemReport> {
    let agent = common::resources::agent_by_id(&agent_id)?;
    let candidate = common::agent_availability::resolve_agent_availability(
        &agent_id,
        common::agent_availability::AgentAvailabilityRequest {
            scan_policy: common::agent_availability::AgentScanPolicy::RefreshIfUnconfigured,
            toolchain_mode: &choices.toolchain_mode,
            candidate_preference:
                common::agent_availability::AgentCandidatePreference::ToolchainMode,
            include_configured_version: true,
        },
    )
    .await
    .ok()?
    .selected?;
    let source = candidate.source.clone();
    let local_version = candidate.version.as_deref().and_then(extract_semver);
    let mut report = StartkitItemReport {
        id: format!("agents.{agent_id}.cli"),
        label: agent.display_name.clone(),
        group: "agents".to_string(),
        category: "agents".to_string(),
        status: StartkitItemStatus::Ok,
        severity: None,
        version: candidate.version.clone(),
        latest_version: None,
        path: Some(candidate.path.clone()),
        message: None,
        actions: Vec::new(),
        manual_command: None,
        manual_url: None,
        secret: false,
        settings_key: None,
    };
    let Some(local_version) = local_version else {
        report.message = Some("Unable to check updates".to_string());
        return Some(report);
    };

    let latest = match tokio::time::timeout(
        AGENT_UPDATE_CHECK_TIMEOUT,
        latest_version_for_agent_source(&agent_id, &source, &choices),
    )
    .await
    {
        Ok(Ok(Some(version))) => version,
        Ok(_) => {
            report.message = Some("Unable to check updates".to_string());
            return Some(report);
        }
        Err(_) => {
            report.message = Some("Update check timed out".to_string());
            return Some(report);
        }
    };

    report.label = agent.display_name.clone();
    report.id = format!("agents.{agent_id}.cli");
    report.latest_version = Some(latest.clone());

    if local_version != latest {
        report.message = Some(format!("Manual update required {latest}"));
    } else {
        report.message = Some("Already up to date".to_string());
    }
    Some(report)
}

async fn latest_version_for_agent_source(
    agent_id: &str,
    source: &str,
    choices: &StartkitChoices,
) -> anyhow::Result<Option<String>> {
    if let Some(package) = agent_detection::source_package(agent_id, source) {
        return npm_latest_version(&package, &choices.source).await;
    }
    if source == "homebrew_formula" || source == "homebrew_cask" {
        return homebrew_latest_version(agent_id, source).await;
    }
    Ok(None)
}

async fn homebrew_latest_version(agent_id: &str, source: &str) -> anyhow::Result<Option<String>> {
    let Some(template) = agent_detection::source_command_template(agent_id, source, "upgrade")
    else {
        return Ok(None);
    };
    let Some(token) = template.split_whitespace().last() else {
        return Ok(None);
    };
    let kind = if source == "homebrew_cask" {
        "--cask"
    } else {
        "--formula"
    };
    let mut command = tokio::process::Command::new("brew");
    command.args(["info", "--json=v2", kind, token]);
    let output = command_output_with_timeout(command, AGENT_UPDATE_CHECK_TIMEOUT)
        .await
        .map_err(anyhow::Error::msg)?;
    if !output.status.success() {
        return Ok(None);
    }
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("parse brew info")?;
    if source == "homebrew_cask" {
        Ok(value
            .get("casks")
            .and_then(|items| items.as_array())
            .and_then(|items| items.first())
            .and_then(|item| item.get("version"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string))
    } else {
        Ok(value
            .get("formulae")
            .and_then(|items| items.as_array())
            .and_then(|items| items.first())
            .and_then(|item| item.get("versions"))
            .and_then(|versions| versions.get("stable"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string))
    }
}

async fn plugin_update_report(plugin_id: String) -> Option<StartkitItemReport> {
    let plugin_def = common::resources::plugin_by_id(&plugin_id)?;
    let discovered = common::plugins::find_user(&plugin_id);
    let local_version = discovered.as_ref().map(|plugin| plugin.installed_version());

    let mut report = StartkitItemReport {
        id: format!("channels.plugins.{plugin_id}"),
        label: plugin_def.name.clone(),
        group: "messaging".to_string(),
        category: "channels".to_string(),
        status: if discovered.is_some() {
            StartkitItemStatus::Ok
        } else {
            StartkitItemStatus::Missing
        },
        severity: None,
        version: local_version.clone(),
        latest_version: None,
        path: discovered
            .as_ref()
            .map(|plugin| plugin.entry_path().to_string_lossy().to_string()),
        message: Some(if discovered.is_some() {
            "Plugin is installed".to_string()
        } else {
            "Plugin is not installed".to_string()
        }),
        actions: if discovered.is_some() {
            Vec::new()
        } else {
            vec!["install".to_string()]
        },
        manual_command: None,
        manual_url: None,
        secret: false,
        settings_key: None,
    };

    let latest = match github_plugin_version(plugin_def).await {
        Ok(Some(version)) => version,
        _ => return Some(report),
    };
    report.latest_version = Some(latest.clone());
    if local_version.as_deref() != Some(latest.as_str()) {
        report.status = if discovered.is_some() {
            StartkitItemStatus::Outdated
        } else {
            StartkitItemStatus::Missing
        };
        report.message = Some(format!("{} {} is available", plugin_def.name, latest));
        report.actions = vec!["install".to_string()];
    } else {
        report.message = Some(format!("{} is up to date", plugin_def.name));
    }
    Some(report)
}

fn program_from_command(command: &str) -> Option<String> {
    command
        .split_whitespace()
        .next()
        .map(|program| program.trim_matches(['"', '\'']).to_string())
        .filter(|program| !program.is_empty())
}

async fn command_output_with_timeout(
    mut command: tokio::process::Command,
    max_duration: Duration,
) -> Result<Output, String> {
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command.kill_on_drop(true);

    let mut child = command.spawn().map_err(|error| error.to_string())?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "stdout was not captured".to_string())?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "stderr was not captured".to_string())?;

    let stdout_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        stdout.read_to_end(&mut buf).await.map(|_| buf)
    });
    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        stderr.read_to_end(&mut buf).await.map(|_| buf)
    });

    let started = Instant::now();
    let status = loop {
        if started.elapsed() >= max_duration {
            let _ = child.kill().await;
            return Err("version check timed out".to_string());
        }
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        sleep(Duration::from_millis(50)).await;
    };

    let stdout = stdout_task
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    let stderr = stderr_task
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;

    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

async fn npm_latest_version(package: &str, source: &str) -> anyhow::Result<Option<String>> {
    if let Some(version) = requested_package_version(package) {
        return Ok(Some(version));
    }

    let package_name = npm_package_name(package);
    let encoded = encode_npm_package_for_url(&package_name);
    let registry = npm_registry_for_source(source);
    let url = format!("{}/{}", registry.trim_end_matches('/'), encoded);
    let client = reqwest::Client::builder()
        .timeout(AGENT_UPDATE_CHECK_TIMEOUT)
        .build()
        .context("build npm metadata client")?;
    let value: serde_json::Value = client
        .get(url)
        .header("accept", "application/vnd.npm.install-v1+json")
        .send()
        .await
        .context("fetch npm package metadata")?
        .error_for_status()
        .context("npm package metadata status")?
        .json()
        .await
        .context("parse npm package metadata")?;
    Ok(value
        .get("dist-tags")
        .and_then(|tags| tags.get("latest"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string))
}

pub(super) async fn github_plugin_version(
    plugin: &common::resources::PluginDef,
) -> anyhow::Result<Option<String>> {
    let Some(package_url) = common::archive::github_revision_raw_file_url(
        &plugin.github,
        &plugin.revision,
        "package.json",
    ) else {
        return Ok(None);
    };
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .context("build plugin metadata client")?;
    if let Some(version) = github_json_version(&client, &package_url).await? {
        return Ok(Some(version));
    }

    let Some(manifest_url) = common::archive::github_revision_raw_file_url(
        &plugin.github,
        &plugin.revision,
        "plugin.json",
    ) else {
        return Ok(None);
    };
    github_json_version(&client, &manifest_url).await
}

async fn github_json_version(
    client: &reqwest::Client,
    url: &str,
) -> anyhow::Result<Option<String>> {
    let response = client
        .get(url)
        .header("accept", "application/json")
        .send()
        .await
        .with_context(|| format!("fetch plugin metadata {url}"))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let value: serde_json::Value = response
        .error_for_status()
        .with_context(|| format!("plugin metadata status {url}"))?
        .json()
        .await
        .with_context(|| format!("parse plugin metadata {url}"))?;
    Ok(value
        .get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|version| !version.is_empty())
        .map(str::to_string))
}

fn npm_registry_for_source(source: &str) -> &'static str {
    match source {
        "cn" => "https://registry.npmmirror.com",
        _ => "https://registry.npmjs.org",
    }
}

fn npm_package_name(package: &str) -> String {
    if let Some(rest) = package.strip_prefix('@') {
        if let Some((scope, name_and_version)) = rest.split_once('/') {
            let name = name_and_version
                .rsplit_once('@')
                .map(|(name, _)| name)
                .unwrap_or(name_and_version);
            return format!("@{scope}/{name}");
        }
    }
    package
        .rsplit_once('@')
        .map(|(name, _)| name)
        .unwrap_or(package)
        .to_string()
}

fn requested_package_version(package: &str) -> Option<String> {
    if let Some(rest) = package.strip_prefix('@') {
        let (_, name_and_version) = rest.split_once('/')?;
        return name_and_version
            .rsplit_once('@')
            .and_then(|(_, version)| (!version.is_empty()).then(|| version.to_string()));
    }
    package
        .rsplit_once('@')
        .and_then(|(_, version)| (!version.is_empty()).then(|| version.to_string()))
}

fn encode_npm_package_for_url(package: &str) -> String {
    package.replace('@', "%40").replace('/', "%2F")
}

fn extract_semver(value: &str) -> Option<String> {
    for token in value.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '.' || ch == '-')) {
        let token = token.trim_start_matches('v');
        let mut parts = token.split('.');
        let major = parts.next()?;
        let minor = parts.next()?;
        let patch = parts.next()?;
        if major.chars().all(|ch| ch.is_ascii_digit())
            && minor.chars().all(|ch| ch.is_ascii_digit())
            && patch.chars().next().is_some_and(|ch| ch.is_ascii_digit())
        {
            return Some(token.to_string());
        }
    }
    None
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
    pub params: Value,
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

async fn stop_plugin_auth(active: PluginAuthSession) {
    let _ = active.cancel.send(true);
    let mut session = active.session.lock().await;
    plugin_session::shutdown_plugin_session(&mut session).await;
}

async fn remove_plugin_auth_if_current(
    state: &OnboardingSessions,
    plugin_id: &str,
    session: &Arc<Mutex<plugin_session::PluginSession>>,
) -> Option<PluginAuthSession> {
    let mut sessions = state.plugin_sessions.lock().await;
    let is_current = sessions
        .get(plugin_id)
        .is_some_and(|active| Arc::ptr_eq(&active.session, session));
    is_current.then(|| sessions.remove(plugin_id)).flatten()
}

#[tauri::command]
pub async fn plugin_auth_start(
    state: State<'_, OnboardingSessions>,
    request: PluginAuthStartRequest,
) -> Result<Value, String> {
    let plugin = plugin_session::resolve_auth_plugin(&request.plugin_id)
        .map_err(|error| error.to_string())?;

    let existing = state
        .plugin_sessions
        .lock()
        .await
        .remove(&request.plugin_id);
    if let Some(existing) = existing {
        stop_plugin_auth(existing).await;
    }

    // TODO: Represent pre-spawn starts in the session map so cancellation or
    // navigation during subprocess initialization can invalidate the result.
    let session = plugin_session::spawn_auth_session(&plugin)
        .await
        .map_err(|error| error.to_string())?;
    let (cancel, cancel_rx) = watch::channel(false);
    let active = PluginAuthSession {
        method: plugin.method,
        session: Arc::new(Mutex::new(session)),
        cancel,
    };
    let displaced = state
        .plugin_sessions
        .lock()
        .await
        .insert(request.plugin_id.clone(), active.clone());
    if let Some(displaced) = displaced {
        stop_plugin_auth(displaced).await;
    }

    let result: anyhow::Result<Value> = {
        let mut session = active.session.lock().await;
        plugin_session::plugin_request(
            &mut session,
            active.method.start_rpc(),
            request.params,
            cancel_rx,
        )
        .await
    };
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            if let Some(active) =
                remove_plugin_auth_if_current(&state, &request.plugin_id, &active.session).await
            {
                stop_plugin_auth(active).await;
            }
            return Err(error.to_string());
        }
    };

    let already_connected = result
        .get("alreadyConnected")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let has_challenge = result
        .get(active.method.challenge_field())
        .and_then(Value::as_str)
        .is_some_and(|value| !value.is_empty());
    if already_connected || !has_challenge {
        if let Some(active) =
            remove_plugin_auth_if_current(&state, &request.plugin_id, &active.session).await
        {
            stop_plugin_auth(active).await;
        }
    }
    Ok(result)
}

#[tauri::command]
pub async fn plugin_auth_wait(
    state: State<'_, OnboardingSessions>,
    request: PluginAuthWaitRequest,
) -> Result<Value, String> {
    let active = state
        .plugin_sessions
        .lock()
        .await
        .get(&request.plugin_id)
        .cloned()
        .ok_or_else(|| format!("auth session for '{}' not started", request.plugin_id))?;

    let result: anyhow::Result<Value> = {
        let mut session = active.session.lock().await;
        plugin_session::plugin_request(
            &mut session,
            active.method.wait_rpc(),
            request.params,
            active.cancel.subscribe(),
        )
        .await
    };
    if let Some(active) =
        remove_plugin_auth_if_current(&state, &request.plugin_id, &active.session).await
    {
        stop_plugin_auth(active).await;
    }

    result.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn plugin_auth_cancel(
    state: State<'_, OnboardingSessions>,
    request: PluginAuthCancelRequest,
) -> Result<(), String> {
    let active = state
        .plugin_sessions
        .lock()
        .await
        .remove(&request.plugin_id);
    if let Some(active) = active {
        stop_plugin_auth(active).await;
    }
    Ok(())
}

/// Marks onboarding complete, signals the daemon gate, and navigates the user
/// to the dashboard.
#[tauri::command]
pub async fn finish_onboarding<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, OnboardingSessions>,
) -> Result<(), String> {
    // Clean up any remaining auth sessions
    let sessions = {
        let mut sessions = state.plugin_sessions.lock().await;
        sessions
            .drain()
            .map(|(_, active)| active)
            .collect::<Vec<_>>()
    };
    for active in sessions {
        stop_plugin_auth(active).await;
    }

    tauri::async_runtime::spawn_blocking(|| {
        config::update_settings_json(|settings| {
            if let Some(obj) = settings.as_object_mut() {
                obj.insert("onboarded".into(), serde_json::json!(true));
            }
        })
    })
    .await
    .map_err(|error| error.to_string())??;

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
