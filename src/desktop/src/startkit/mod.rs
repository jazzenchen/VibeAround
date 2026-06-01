//! Startkit: manifest-driven environment doctor and repair runner.
//!
//! The manifest and scripts live under `src/resources/startkit/`. This module
//! keeps the engine generic: resolve the item graph from user choices, execute
//! platform scripts with a stable environment, and normalize all output into
//! structured item reports for the onboarding UI.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Runtime, State};
use tokio::process::Command;
use tokio::time::timeout;

const SETTINGS_TOML: &str = include_str!("../../../resources/startkit/settings.toml");
const STARTKIT_PROGRESS_EVENT: &str = "startkit-progress";
const STARTKIT_COMPLETE_EVENT: &str = "startkit-complete";

pub struct StartkitRunState {
    cancelled: Arc<AtomicBool>,
}

impl Default for StartkitRunState {
    fn default() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub startkit: StartkitInfo,
    #[serde(default)]
    pub runner: RunnerConfig,
    #[serde(default)]
    pub sources: HashMap<String, SourceConfig>,
    #[serde(default, rename = "items")]
    pub items: Vec<StartkitItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StartkitInfo {
    pub id: String,
    pub name: String,
    pub schema: u32,
    pub version: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RunnerConfig {
    #[serde(default = "default_timeout_secs")]
    pub default_timeout_secs: u64,
    #[serde(default)]
    pub log_redact_keys: Vec<String>,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            default_timeout_secs: default_timeout_secs(),
            log_redact_keys: Vec::new(),
        }
    }
}

fn default_timeout_secs() -> u64 {
    600
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SourceConfig {
    pub label: String,
    pub node_index: String,
    pub node_dist: String,
    pub npm_registry: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StartkitItem {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub include_if: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub severity: Option<String>,
    #[serde(default)]
    pub managed: bool,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub min_version: Option<String>,
    #[serde(default)]
    pub program: Option<String>,
    #[serde(default)]
    pub version_arg: Option<String>,
    #[serde(default)]
    pub npm_package: Option<String>,
    #[serde(default)]
    pub settings_key: Option<String>,
    #[serde(default)]
    pub secret: bool,
    #[serde(default)]
    pub detect: Option<PlatformScript>,
    #[serde(default)]
    pub install: Option<PlatformScript>,
    #[serde(default)]
    pub repair: Option<PlatformScript>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlatformScript {
    #[serde(default)]
    pub macos: Option<String>,
    #[serde(default)]
    pub windows: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
}

impl PlatformScript {
    fn for_platform(&self, platform: &str) -> Option<&str> {
        match platform {
            "macos" => self.macos.as_deref(),
            "windows" => self.windows.as_deref(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartkitChoices {
    #[serde(default)]
    pub agents: Vec<String>,
    #[serde(default = "default_tunnel")]
    pub tunnel: String,
    #[serde(default)]
    pub channels: Vec<String>,
    #[serde(default = "default_source")]
    pub source: String,
}

fn default_tunnel() -> String {
    "none".to_string()
}

fn default_source() -> String {
    "global".to_string()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartkitManifestSummary {
    pub id: String,
    pub name: String,
    pub schema: u32,
    pub version: String,
    pub sources: HashMap<String, SourceConfig>,
    pub items: Vec<StartkitItemSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartkitItemSummary {
    pub id: String,
    pub label: String,
    pub group: String,
    pub category: String,
    pub description: Option<String>,
    pub severity: Option<String>,
    pub kind: Option<String>,
    pub managed: bool,
    pub has_repair: bool,
    pub secret: bool,
    pub settings_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartkitPlan {
    pub platform: String,
    pub source: String,
    pub item_ids: Vec<String>,
    pub items: Vec<StartkitItemSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartkitItemStatus {
    Pending,
    Running,
    Ok,
    Missing,
    Outdated,
    Broken,
    NeedsConfig,
    Blocked,
    Error,
    Skipped,
}

impl StartkitItemStatus {
    fn from_script(value: &str) -> Self {
        match value {
            "ok" => Self::Ok,
            "missing" => Self::Missing,
            "outdated" => Self::Outdated,
            "broken" => Self::Broken,
            "needs_config" => Self::NeedsConfig,
            "blocked" => Self::Blocked,
            "skipped" => Self::Skipped,
            _ => Self::Error,
        }
    }

    fn needs_install(&self) -> bool {
        matches!(self, Self::Missing | Self::Outdated | Self::Broken)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartkitItemReport {
    pub id: String,
    pub label: String,
    pub group: String,
    pub category: String,
    pub status: StartkitItemStatus,
    pub severity: Option<String>,
    pub version: Option<String>,
    pub path: Option<String>,
    pub message: Option<String>,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default)]
    pub secret: bool,
    #[serde(default)]
    pub settings_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartkitScanReport {
    pub plan: StartkitPlan,
    pub reports: Vec<StartkitItemReport>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartkitProgressEvent {
    pub id: String,
    pub label: String,
    pub status: StartkitItemStatus,
    pub message: Option<String>,
    pub report: Option<StartkitItemReport>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartkitCompleteEvent {
    pub status: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ScriptOutput {
    status: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    actions: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct StartkitPaths {
    pub root: PathBuf,
    pub home: PathBuf,
    pub bin_dir: PathBuf,
    pub runtime_dir: PathBuf,
    pub node_dir: PathBuf,
    pub npm_prefix: PathBuf,
    pub cache_dir: PathBuf,
}

impl StartkitPaths {
    pub fn new(root: PathBuf) -> Self {
        let home = common::config::data_dir();
        let runtime_dir = home.join("runtime");
        Self {
            root,
            bin_dir: home.join("bin"),
            node_dir: runtime_dir.join("node"),
            npm_prefix: home.join("npm-global"),
            cache_dir: home.join("cache").join("startkit"),
            runtime_dir,
            home,
        }
    }
}

pub fn load_manifest() -> anyhow::Result<Manifest> {
    toml::from_str(SETTINGS_TOML).context("parsing startkit/settings.toml")
}

#[tauri::command]
pub fn startkit_manifest() -> Result<StartkitManifestSummary, String> {
    manifest_summary().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn startkit_plan(choices: StartkitChoices) -> Result<StartkitPlan, String> {
    plan(&choices, None).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn startkit_scan(
    settings: Value,
    choices: StartkitChoices,
) -> Result<StartkitScanReport, String> {
    scan(&settings, &choices, None)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn start_startkit_install<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, StartkitRunState>,
    settings: Value,
    choices: StartkitChoices,
) -> Result<(), String> {
    state.cancelled.store(false, Ordering::Relaxed);
    common::config::write_settings_json(&settings).map_err(|e| e.to_string())?;

    let cancelled = Arc::clone(&state.cancelled);
    tauri::async_runtime::spawn(async move {
        let status = match run_startkit_install(app.clone(), settings, choices, cancelled).await {
            Ok(status) => status,
            Err(err) => {
                let _ = app.emit(
                    STARTKIT_PROGRESS_EVENT,
                    StartkitProgressEvent {
                        id: "startkit".to_string(),
                        label: "Startkit".to_string(),
                        status: StartkitItemStatus::Error,
                        message: Some(err.to_string()),
                        report: None,
                    },
                );
                "error".to_string()
            }
        };
        let _ = app.emit(STARTKIT_COMPLETE_EVENT, StartkitCompleteEvent { status });
    });

    Ok(())
}

#[tauri::command]
pub async fn cancel_startkit_install(state: State<'_, StartkitRunState>) -> Result<(), String> {
    state.cancelled.store(true, Ordering::Relaxed);
    Ok(())
}

pub fn manifest_summary() -> anyhow::Result<StartkitManifestSummary> {
    let manifest = load_manifest()?;
    Ok(StartkitManifestSummary {
        id: manifest.startkit.id,
        name: manifest.startkit.name,
        schema: manifest.startkit.schema,
        version: manifest.startkit.version,
        sources: manifest.sources,
        items: manifest.items.iter().map(item_summary).collect(),
    })
}

pub fn plan(choices: &StartkitChoices, platform: Option<&str>) -> anyhow::Result<StartkitPlan> {
    let manifest = load_manifest()?;
    plan_from_manifest(&manifest, choices, platform.unwrap_or(current_platform()))
}

pub async fn scan(
    settings: &Value,
    choices: &StartkitChoices,
    platform: Option<&str>,
) -> anyhow::Result<StartkitScanReport> {
    let manifest = load_manifest()?;
    let platform = platform.unwrap_or(current_platform());
    let plan = plan_from_manifest(&manifest, choices, platform)?;
    let paths = StartkitPaths::new(startkit_root());
    let mut reports = Vec::new();

    for item_id in &plan.item_ids {
        let item = find_item(&manifest, item_id)?;
        reports.push(scan_item(&manifest, &paths, item, settings, choices, platform).await);
    }

    Ok(StartkitScanReport { plan, reports })
}

pub async fn execute_item(
    settings: &Value,
    choices: &StartkitChoices,
    item_id: &str,
) -> anyhow::Result<StartkitItemReport> {
    let manifest = load_manifest()?;
    let platform = current_platform();
    let paths = StartkitPaths::new(startkit_root());
    let item = find_item(&manifest, item_id)?;
    let before = scan_item(&manifest, &paths, item, settings, choices, platform).await;

    if !before.status.needs_install() {
        return Ok(before);
    }

    let Some(script) = &item.install else {
        return Ok(StartkitItemReport {
            status: StartkitItemStatus::Blocked,
            message: Some("No automatic install action is available".to_string()),
            ..base_report(item)
        });
    };

    let Some(script_path) = script.for_platform(platform) else {
        return Ok(StartkitItemReport {
            status: StartkitItemStatus::Blocked,
            message: Some(format!("No install script for {platform}")),
            ..base_report(item)
        });
    };

    match run_script(&manifest, &paths, item, choices, platform, script_path, script).await {
        Ok(report) => report_from_script(item, report),
        Err(err) => StartkitItemReport {
            status: StartkitItemStatus::Error,
            message: Some(err.to_string()),
            ..base_report(item)
        },
    }
    .pipe(Ok)
}

async fn run_startkit_install<R: Runtime>(
    app: AppHandle<R>,
    settings: Value,
    choices: StartkitChoices,
    cancelled: Arc<AtomicBool>,
) -> anyhow::Result<String> {
    let manifest = load_manifest()?;
    let platform = current_platform();
    let plan = plan_from_manifest(&manifest, &choices, platform)?;
    let mut had_error = false;
    let mut needs_input = false;

    for item_id in &plan.item_ids {
        if cancelled.load(Ordering::Relaxed) {
            return Ok("cancelled".to_string());
        }

        let item = find_item(&manifest, item_id)?;
        emit_progress(
            &app,
            item,
            StartkitItemStatus::Running,
            Some("Checking".to_string()),
            None,
        );

        let report = if item.kind.as_deref() == Some("builtin_channel_plugins") {
            run_channel_plugins_item(&app, item, &settings, &choices, &cancelled).await
        } else {
            execute_item(&settings, &choices, item_id).await
        };

        match report {
            Ok(report) => {
                if matches!(report.status, StartkitItemStatus::Error | StartkitItemStatus::Blocked)
                {
                    had_error = true;
                }
                if matches!(report.status, StartkitItemStatus::NeedsConfig) {
                    needs_input = true;
                }
                emit_progress(
                    &app,
                    item,
                    report.status.clone(),
                    report.message.clone(),
                    Some(report),
                );
            }
            Err(err) => {
                had_error = true;
                emit_progress(
                    &app,
                    item,
                    StartkitItemStatus::Error,
                    Some(err.to_string()),
                    None,
                );
            }
        }
    }

    Ok(if cancelled.load(Ordering::Relaxed) {
        "cancelled"
    } else if had_error {
        "error"
    } else if needs_input {
        "needs_input"
    } else {
        "complete"
    }
    .to_string())
}

async fn run_channel_plugins_item<R: Runtime>(
    app: &AppHandle<R>,
    item: &StartkitItem,
    _settings: &Value,
    choices: &StartkitChoices,
    cancelled: &Arc<AtomicBool>,
) -> anyhow::Result<StartkitItemReport> {
    if choices.channels.is_empty() {
        return Ok(StartkitItemReport {
            status: StartkitItemStatus::Skipped,
            message: Some("No channel plugins selected".to_string()),
            ..base_report(item)
        });
    }

    for agent_id in &choices.agents {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(StartkitItemReport {
                status: StartkitItemStatus::Skipped,
                message: Some("Cancelled".to_string()),
                ..base_report(item)
            });
        }
        install_acp_adapter_for_agent(app, item, agent_id, cancelled).await?;
    }

    for channel_id in &choices.channels {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(StartkitItemReport {
                status: StartkitItemStatus::Skipped,
                message: Some("Cancelled".to_string()),
                ..base_report(item)
            });
        }
        install_channel_plugin(app, item, channel_id, cancelled).await?;
    }

    Ok(StartkitItemReport {
        status: StartkitItemStatus::Ok,
        message: Some("Channel plugins are ready".to_string()),
        actions: Vec::new(),
        ..base_report(item)
    })
}

async fn install_acp_adapter_for_agent<R: Runtime>(
    app: &AppHandle<R>,
    item: &StartkitItem,
    agent_id: &str,
    cancelled: &Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let Some(agent_def) = common::resources::agent_by_id(agent_id) else {
        return Ok(());
    };
    let Some(npm_pkg) = agent_def.acp.npm_package.as_deref() else {
        return Ok(());
    };
    let default_bin_name = common::agent::npm_package_bin_name(npm_pkg);
    let bin_name = agent_def
        .acp
        .bin_name
        .as_deref()
        .unwrap_or(&default_bin_name);

    if common::agent::npm_package_installed(npm_pkg, bin_name) {
        emit_progress(
            app,
            item,
            StartkitItemStatus::Running,
            Some(format!("{} ACP adapter already installed", agent_def.display_name)),
            None,
        );
        return Ok(());
    }

    emit_progress(
        app,
        item,
        StartkitItemStatus::Running,
        Some(format!("Installing {} ACP adapter", agent_def.display_name)),
        None,
    );

    common::agent::auto_install_npm_agent_with_progress_and_cancel(
        npm_pkg,
        |line| {
            emit_progress(
                app,
                item,
                StartkitItemStatus::Running,
                Some(line),
                None,
            );
        },
        || cancelled.load(Ordering::Relaxed),
    )
    .await
    .map(|_| ())
}

async fn install_channel_plugin<R: Runtime>(
    app: &AppHandle<R>,
    item: &StartkitItem,
    channel_id: &str,
    cancelled: &Arc<AtomicBool>,
) -> anyhow::Result<()> {
    if crate::onboarding::check_plugin_status(channel_id.to_string()) == "ready" {
        emit_progress(
            app,
            item,
            StartkitItemStatus::Running,
            Some(format!("{channel_id} plugin already installed")),
            None,
        );
        return Ok(());
    }

    let plugin = common::resources::plugin_by_id(channel_id)
        .ok_or_else(|| anyhow!("channel plugin '{channel_id}' not found in registry"))?;

    emit_progress(
        app,
        item,
        StartkitItemStatus::Running,
        Some(format!("Installing {} plugin", plugin.name)),
        None,
    );

    crate::onboarding::plugin_install::run_install_inner_with_progress(
        crate::onboarding::plugin_install::InstallPluginRequest {
            plugin_id: channel_id.to_string(),
            github_url: plugin.github.clone(),
        },
        |line| {
            emit_progress(
                app,
                item,
                StartkitItemStatus::Running,
                Some(line),
                None,
            );
        },
        || cancelled.load(Ordering::Relaxed),
    )
    .await
    .map(|_| ())
}

fn emit_progress<R: Runtime>(
    app: &AppHandle<R>,
    item: &StartkitItem,
    status: StartkitItemStatus,
    message: Option<String>,
    report: Option<StartkitItemReport>,
) {
    let _ = app.emit(
        STARTKIT_PROGRESS_EVENT,
        StartkitProgressEvent {
            id: item.id.clone(),
            label: item.label.clone(),
            status,
            message,
            report,
        },
    );
}

async fn scan_item(
    manifest: &Manifest,
    paths: &StartkitPaths,
    item: &StartkitItem,
    settings: &Value,
    choices: &StartkitChoices,
    platform: &str,
) -> StartkitItemReport {
    if item.kind.as_deref() == Some("config") {
        return scan_config_item(item, settings);
    }

    if item.kind.as_deref() == Some("builtin_channel_plugins") {
        let status = if choices.channels.is_empty() {
            StartkitItemStatus::Skipped
        } else {
            StartkitItemStatus::Pending
        };
        return StartkitItemReport {
            status,
            message: Some(format!("{} channel plugin(s) selected", choices.channels.len())),
            actions: if choices.channels.is_empty() {
                Vec::new()
            } else {
                vec!["install".to_string()]
            },
            ..base_report(item)
        };
    }

    let Some(detect) = &item.detect else {
        return StartkitItemReport {
            status: StartkitItemStatus::Pending,
            message: Some("No detect script configured".to_string()),
            ..base_report(item)
        };
    };

    let Some(script_path) = detect.for_platform(platform) else {
        return StartkitItemReport {
            status: StartkitItemStatus::Blocked,
            message: Some(format!("No detect script for {platform}")),
            ..base_report(item)
        };
    };

    match run_script(manifest, paths, item, choices, platform, script_path, detect).await {
        Ok(output) => report_from_script(item, output),
        Err(err) => StartkitItemReport {
            status: StartkitItemStatus::Error,
            message: Some(err.to_string()),
            ..base_report(item)
        },
    }
}

fn scan_config_item(item: &StartkitItem, settings: &Value) -> StartkitItemReport {
    let has_value = item
        .settings_key
        .as_deref()
        .and_then(|key| settings_path_value(settings, key))
        .and_then(Value::as_str)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);

    StartkitItemReport {
        status: if has_value {
            StartkitItemStatus::Ok
        } else {
            StartkitItemStatus::NeedsConfig
        },
        message: Some(if has_value {
            "Configured".to_string()
        } else {
            "Needs configuration".to_string()
        }),
        actions: if has_value {
            Vec::new()
        } else {
            vec!["configure".to_string()]
        },
        ..base_report(item)
    }
}

async fn run_script(
    manifest: &Manifest,
    paths: &StartkitPaths,
    item: &StartkitItem,
    choices: &StartkitChoices,
    platform: &str,
    script_path: &str,
    script: &PlatformScript,
) -> anyhow::Result<ScriptOutput> {
    let full_path = paths.root.join(script_path);
    if !full_path.exists() {
        bail!("script not found: {}", full_path.display());
    }

    let mut command = if platform == "windows" {
        let mut cmd = Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
        cmd.arg(&full_path);
        cmd
    } else {
        let mut cmd = Command::new("sh");
        cmd.arg(&full_path);
        cmd
    };

    command.args(&script.args);
    command.env_clear();
    command.envs(common::process::env::enriched_env().clone());
    apply_startkit_env(&mut command, manifest, paths, item, choices)?;
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());

    let output = timeout(
        Duration::from_secs(manifest.runner.default_timeout_secs),
        command.output(),
    )
    .await
    .context("startkit script timed out")?
    .context("running startkit script")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let line = stdout
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with('{'))
        .ok_or_else(|| {
            anyhow!(
                "script did not emit JSON{}",
                if stderr.trim().is_empty() {
                    String::new()
                } else {
                    format!(": {}", redact(&stderr, &manifest.runner.log_redact_keys))
                }
            )
        })?;

    let parsed: ScriptOutput =
        serde_json::from_str(line).with_context(|| format!("parsing script JSON: {line}"))?;
    Ok(parsed)
}

fn apply_startkit_env(
    command: &mut Command,
    manifest: &Manifest,
    paths: &StartkitPaths,
    item: &StartkitItem,
    choices: &StartkitChoices,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(&paths.bin_dir).ok();
    std::fs::create_dir_all(&paths.runtime_dir).ok();
    std::fs::create_dir_all(&paths.cache_dir).ok();

    let source = manifest
        .sources
        .get(&choices.source)
        .or_else(|| manifest.sources.get("global"))
        .ok_or_else(|| anyhow!("startkit source '{}' not found", choices.source))?;

    let managed_paths = managed_path_entries();
    let current_path = common::process::env::enriched_env()
        .get("PATH")
        .cloned()
        .unwrap_or_default();
    let sep = if cfg!(windows) { ";" } else { ":" };
    let mut path = managed_paths
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    path.push(current_path);

    command.env("PATH", path.join(sep));
    command.env("STARTKIT_HOME", &paths.home);
    command.env("STARTKIT_ROOT", &paths.root);
    command.env("STARTKIT_BIN_DIR", &paths.bin_dir);
    command.env("STARTKIT_RUNTIME_DIR", &paths.runtime_dir);
    command.env("STARTKIT_NODE_DIR", &paths.node_dir);
    command.env("STARTKIT_NPM_PREFIX", &paths.npm_prefix);
    command.env("STARTKIT_CACHE_DIR", &paths.cache_dir);
    command.env("STARTKIT_SOURCE", &choices.source);
    command.env("STARTKIT_NODE_INDEX_URL", &source.node_index);
    command.env("STARTKIT_NODE_DIST_BASE", &source.node_dist);
    command.env("STARTKIT_NPM_REGISTRY", &source.npm_registry);
    command.env("STARTKIT_ITEM_ID", &item.id);
    if let Some(value) = &item.min_version {
        command.env("STARTKIT_MIN_VERSION", value);
    }
    if let Some(value) = &item.program {
        command.env("STARTKIT_PROGRAM", value);
    }
    if let Some(value) = &item.version_arg {
        command.env("STARTKIT_VERSION_ARG", value);
    }
    if let Some(value) = &item.npm_package {
        command.env("STARTKIT_NPM_PACKAGE", value);
    }

    Ok(())
}

pub fn managed_path_entries() -> Vec<PathBuf> {
    let home = common::config::data_dir();
    let mut entries = Vec::new();
    entries.push(home.join("bin"));
    entries.push(home.join("runtime").join("node").join("bin"));
    entries.push(home.join("runtime").join("node"));
    entries.push(home.join("npm-global").join("bin"));
    entries.push(home.join("npm-global"));
    entries
}

fn report_from_script(item: &StartkitItem, output: ScriptOutput) -> StartkitItemReport {
    StartkitItemReport {
        status: StartkitItemStatus::from_script(&output.status),
        version: output.version,
        path: output.path,
        message: output.message,
        actions: output.actions,
        ..base_report(item)
    }
}

fn base_report(item: &StartkitItem) -> StartkitItemReport {
    StartkitItemReport {
        id: item.id.clone(),
        label: item.label.clone(),
        group: item.group.clone(),
        category: item.category.clone(),
        status: StartkitItemStatus::Pending,
        severity: item.severity.clone(),
        version: None,
        path: None,
        message: None,
        actions: Vec::new(),
        secret: item.secret,
        settings_key: item.settings_key.clone(),
    }
}

fn item_summary(item: &StartkitItem) -> StartkitItemSummary {
    StartkitItemSummary {
        id: item.id.clone(),
        label: item.label.clone(),
        group: item.group.clone(),
        category: item.category.clone(),
        description: item.description.clone(),
        severity: item.severity.clone(),
        kind: item.kind.clone(),
        managed: item.managed,
        has_repair: item.repair.is_some(),
        secret: item.secret,
        settings_key: item.settings_key.clone(),
    }
}

fn plan_from_manifest(
    manifest: &Manifest,
    choices: &StartkitChoices,
    platform: &str,
) -> anyhow::Result<StartkitPlan> {
    let by_id: HashMap<&str, &StartkitItem> = manifest
        .items
        .iter()
        .map(|item| (item.id.as_str(), item))
        .collect();
    let mut selected = HashSet::<String>::new();

    for item in &manifest.items {
        if !supports_platform(item, platform) {
            continue;
        }
        if should_include(item, choices) {
            add_with_deps(item, &by_id, platform, &mut selected)?;
        }
    }

    let mut ordered = Vec::new();
    let mut temporary = HashSet::new();
    let mut permanent = HashSet::new();
    for id in selected.iter() {
        visit(id, &by_id, platform, &selected, &mut temporary, &mut permanent, &mut ordered)?;
    }

    let items = ordered
        .iter()
        .map(|id| item_summary(find_item(manifest, id).expect("planned item exists")))
        .collect();

    Ok(StartkitPlan {
        platform: platform.to_string(),
        source: choices.source.clone(),
        item_ids: ordered,
        items,
    })
}

fn add_with_deps(
    item: &StartkitItem,
    by_id: &HashMap<&str, &StartkitItem>,
    platform: &str,
    selected: &mut HashSet<String>,
) -> anyhow::Result<()> {
    selected.insert(item.id.clone());
    for dep in &item.depends_on {
        let dep_item = by_id
            .get(dep.as_str())
            .ok_or_else(|| anyhow!("startkit item '{}' depends on missing '{}'", item.id, dep))?;
        if !supports_platform(dep_item, platform) {
            continue;
        }
        add_with_deps(dep_item, by_id, platform, selected)?;
    }
    Ok(())
}

fn visit(
    id: &str,
    by_id: &HashMap<&str, &StartkitItem>,
    platform: &str,
    selected: &HashSet<String>,
    temporary: &mut HashSet<String>,
    permanent: &mut HashSet<String>,
    ordered: &mut Vec<String>,
) -> anyhow::Result<()> {
    if permanent.contains(id) {
        return Ok(());
    }
    if !temporary.insert(id.to_string()) {
        bail!("cycle in startkit item dependencies at '{id}'");
    }
    let item = by_id
        .get(id)
        .ok_or_else(|| anyhow!("planned startkit item missing: {id}"))?;
    for dep in &item.depends_on {
        if selected.contains(dep) {
            let dep_item = by_id
                .get(dep.as_str())
                .ok_or_else(|| anyhow!("startkit item '{}' depends on missing '{}'", item.id, dep))?;
            if supports_platform(dep_item, platform) {
                visit(dep, by_id, platform, selected, temporary, permanent, ordered)?;
            }
        }
    }
    temporary.remove(id);
    permanent.insert(id.to_string());
    ordered.push(id.to_string());
    Ok(())
}

fn should_include(item: &StartkitItem, choices: &StartkitChoices) -> bool {
    item.include_if.iter().any(|rule| match rule.as_str() {
        "always" => true,
        "agent:any" => !choices.agents.is_empty(),
        "channels:any" => !choices.channels.is_empty(),
        "tunnel:any" => choices.tunnel != "none",
        rule if rule.starts_with("agent:") => {
            let agent = &rule["agent:".len()..];
            choices.agents.iter().any(|id| id == agent)
        }
        rule if rule.starts_with("tunnel:") => {
            let tunnel = &rule["tunnel:".len()..];
            choices.tunnel == tunnel
        }
        _ => false,
    })
}

fn supports_platform(item: &StartkitItem, platform: &str) -> bool {
    item.platforms.is_empty() || item.platforms.iter().any(|p| p == platform)
}

fn find_item<'a>(manifest: &'a Manifest, id: &str) -> anyhow::Result<&'a StartkitItem> {
    manifest
        .items
        .iter()
        .find(|item| item.id == id)
        .ok_or_else(|| anyhow!("unknown startkit item: {id}"))
}

fn settings_path_value<'a>(settings: &'a Value, key: &str) -> Option<&'a Value> {
    let mut current = settings;
    for part in key.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

pub fn current_platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        other => other,
    }
}

fn startkit_root() -> PathBuf {
    #[cfg(debug_assertions)]
    {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../resources/startkit")
    }
    #[cfg(not(debug_assertions))]
    {
        // Best-effort fallback for release bundles. IPC entry points that have
        // an AppHandle can swap this for app.path().resource_dir() later; this
        // keeps the planner/test layer independent of Tauri state.
        std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."))
            .join("_up_")
            .join("resources")
            .join("startkit")
    }
}

fn redact(value: &str, keys: &[String]) -> String {
    let mut out = value.to_string();
    for key in keys {
        if key.is_empty() {
            continue;
        }
        out = out.replace(key, "***");
    }
    out
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}

impl<T> Pipe for T {}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(choices: StartkitChoices) -> Vec<String> {
        let manifest = load_manifest().unwrap();
        plan_from_manifest(&manifest, &choices, "macos")
            .unwrap()
            .item_ids
    }

    #[test]
    fn codex_only_plan_does_not_install_claude_or_tunnel() {
        let item_ids = ids(StartkitChoices {
            agents: vec!["codex".to_string()],
            tunnel: "none".to_string(),
            channels: Vec::new(),
            source: "global".to_string(),
        });

        assert!(item_ids.contains(&"essentials.node".to_string()));
        assert!(item_ids.contains(&"essentials.git".to_string()));
        assert!(item_ids.contains(&"agents.codex.cli".to_string()));
        assert!(!item_ids.contains(&"agents.claude.cli".to_string()));
        assert!(!item_ids.contains(&"tunnels.cloudflare.binary".to_string()));
    }

    #[test]
    fn cloudflare_plan_includes_binary_and_config_without_agents() {
        let item_ids = ids(StartkitChoices {
            agents: Vec::new(),
            tunnel: "cloudflare".to_string(),
            channels: Vec::new(),
            source: "cn".to_string(),
        });

        assert_eq!(
            item_ids,
            vec![
                "tunnels.cloudflare.binary",
                "tunnels.cloudflare.token",
                "tunnels.cloudflare.hostname"
            ]
        );
    }

    #[test]
    fn channels_pull_node_and_git() {
        let item_ids = ids(StartkitChoices {
            agents: Vec::new(),
            tunnel: "none".to_string(),
            channels: vec!["telegram".to_string()],
            source: "global".to_string(),
        });

        assert!(item_ids.contains(&"essentials.node".to_string()));
        assert!(item_ids.contains(&"essentials.git".to_string()));
        assert!(item_ids.contains(&"channels.plugins".to_string()));
    }
}
