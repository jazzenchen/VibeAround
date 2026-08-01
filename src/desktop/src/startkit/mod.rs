//! Startkit: manifest-driven environment doctor and repair runner.
//!
//! The manifest and scripts live under `src/resources/startkit/`. This module
//! keeps the engine generic: resolve the item graph from user choices, execute
//! platform scripts with a stable environment, and normalize all output into
//! structured item reports for the onboarding UI.

mod agent;
mod channels;
mod managed;
mod plan;
mod redact;
mod script;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Runtime, State};
use tokio::task::JoinSet;

#[cfg(test)]
use agent::agent_cli_npm_install_package;
use agent::{agent_id_from_cli_item, execute_agent_cli_item, scan_agent_cli_item};
use channels::run_channel_plugins_item;
use managed::{
    execute_managed_toolchain_item, item_uses_managed_dependency_dir, run_managed_npm_package_item,
    scan_managed_npm_package_item, scan_managed_toolchain_item,
};
use plan::{
    effective_item_dependencies, find_item, is_managed_mode, item_summary, plan_from_manifest,
    portable_toolchain_enabled,
};
use script::{run_script, ScriptOutput};

const SETTINGS_TOML: &str = include_str!("../../../resources/startkit/settings.toml");
const STARTKIT_PROGRESS_EVENT: &str = "startkit-progress";
const STARTKIT_COMPLETE_EVENT: &str = "startkit-complete";
const STARTKIT_ITEM_SCAN_TIMEOUT: Duration = Duration::from_secs(8);

pub(in crate::startkit) type StartkitProgress<'a> =
    Option<&'a (dyn Fn(&StartkitItem, StartkitItemStatus, Option<String>) + Sync)>;

pub struct StartkitRunState {
    active: Arc<Mutex<Option<Arc<StartkitRunControl>>>>,
}

struct StartkitRunControl {
    run_id: String,
    cancelled: Arc<AtomicBool>,
}

impl Default for StartkitRunState {
    fn default() -> Self {
        Self {
            active: Arc::new(Mutex::new(None)),
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
    pub plugin_dependency: Option<String>,
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
    pub linux: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
}

impl PlatformScript {
    fn for_platform(&self, platform: &str) -> Option<&str> {
        match platform {
            "macos" => self.macos.as_deref(),
            "windows" => self.windows.as_deref(),
            "linux" => self.linux.as_deref(),
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
    #[serde(default = "default_toolchain_mode")]
    pub toolchain_mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub portable_toolchain: Option<bool>,
    #[serde(default)]
    pub shell_path: bool,
}

impl Default for StartkitChoices {
    fn default() -> Self {
        Self {
            agents: Vec::new(),
            tunnel: default_tunnel(),
            channels: Vec::new(),
            source: default_source(),
            toolchain_mode: default_toolchain_mode(),
            portable_toolchain: Some(false),
            shell_path: false,
        }
    }
}

fn default_tunnel() -> String {
    "none".to_string()
}

fn default_source() -> String {
    "global".to_string()
}

fn default_toolchain_mode() -> String {
    "system".to_string()
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_version: Option<String>,
    pub path: Option<String>,
    pub message: Option<String>,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual_url: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub id: String,
    pub label: String,
    pub status: StartkitItemStatus,
    pub message: Option<String>,
    pub report: Option<StartkitItemReport>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartkitCompleteEvent {
    pub run_id: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct StartkitPaths {
    pub root: PathBuf,
    pub home: PathBuf,
    pub cache_dir: PathBuf,
}

impl StartkitPaths {
    pub fn new(root: PathBuf) -> Self {
        let home = common::config::data_dir();
        Self {
            root,
            cache_dir: home.join("cache").join("startkit"),
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
pub async fn startkit_scan<R: Runtime>(
    app: AppHandle<R>,
    settings: Value,
    choices: StartkitChoices,
) -> Result<StartkitScanReport, String> {
    scan_with_progress(Some(&app), &settings, &choices, None)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn start_startkit_install<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, StartkitRunState>,
    settings_patch: Value,
    choices: StartkitChoices,
    run_id: Option<String>,
    skipped_item_ids: Option<Vec<String>>,
) -> Result<Value, String> {
    let settings = tauri::async_runtime::spawn_blocking(move || {
        common::config::patch_settings_json(&settings_patch)
    })
    .await
    .map_err(|error| error.to_string())??
    .settings;

    let run_id = normalize_run_id(run_id);
    let control = Arc::new(StartkitRunControl {
        run_id: run_id.clone(),
        cancelled: Arc::new(AtomicBool::new(false)),
    });
    {
        let mut active = state
            .active
            .lock()
            .map_err(|_| "startkit run state lock poisoned".to_string())?;
        if let Some(previous) = active.replace(Arc::clone(&control)) {
            previous.cancelled.store(true, Ordering::Relaxed);
        }
    }

    let active = Arc::clone(&state.active);
    let install_settings = settings.clone();
    let skipped_item_ids = skipped_item_ids
        .unwrap_or_default()
        .into_iter()
        .collect::<HashSet<_>>();
    tauri::async_runtime::spawn(async move {
        let status = match run_startkit_install(
            app.clone(),
            install_settings,
            choices,
            Arc::clone(&control.cancelled),
            run_id.clone(),
            skipped_item_ids,
        )
        .await
        {
            Ok(status) => status,
            Err(err) => {
                if is_active_run(&active, &run_id) {
                    let _ = app.emit(
                        STARTKIT_PROGRESS_EVENT,
                        StartkitProgressEvent {
                            run_id: Some(run_id.clone()),
                            id: "startkit".to_string(),
                            label: "Startkit".to_string(),
                            status: StartkitItemStatus::Error,
                            message: Some(err.to_string()),
                            report: None,
                        },
                    );
                }
                "error".to_string()
            }
        };
        if finish_active_run(&active, &run_id) {
            let _ = app.emit(
                STARTKIT_COMPLETE_EVENT,
                StartkitCompleteEvent { run_id, status },
            );
        }
    });

    Ok(settings)
}

#[tauri::command]
pub async fn cancel_startkit_install(state: State<'_, StartkitRunState>) -> Result<(), String> {
    if let Some(control) = state
        .active
        .lock()
        .map_err(|_| "startkit run state lock poisoned".to_string())?
        .as_ref()
    {
        control.cancelled.store(true, Ordering::Relaxed);
    }
    Ok(())
}

fn normalize_run_id(run_id: Option<String>) -> String {
    run_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
}

fn is_active_run(active: &Arc<Mutex<Option<Arc<StartkitRunControl>>>>, run_id: &str) -> bool {
    active
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(|control| control.run_id == run_id))
        .unwrap_or(false)
}

fn finish_active_run(active: &Arc<Mutex<Option<Arc<StartkitRunControl>>>>, run_id: &str) -> bool {
    let Ok(mut guard) = active.lock() else {
        return false;
    };
    let matches = guard
        .as_ref()
        .map(|control| control.run_id == run_id)
        .unwrap_or(false);
    if matches {
        *guard = None;
    }
    matches
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

#[allow(dead_code)]
pub async fn scan(
    settings: &Value,
    choices: &StartkitChoices,
    platform: Option<&str>,
) -> anyhow::Result<StartkitScanReport> {
    scan_with_progress::<tauri::Wry>(None, settings, choices, platform).await
}

pub(crate) async fn scan_agent_cli_reports(
    settings: &Value,
    choices: &StartkitChoices,
    agent_ids: &[String],
) -> anyhow::Result<Vec<StartkitItemReport>> {
    let _ = settings;
    let manifest = load_manifest()?;
    let mut tasks = JoinSet::new();

    for (index, agent_id) in agent_ids.iter().enumerate() {
        let item_id = format!("agents.{agent_id}.cli");
        let Ok(item) = find_item(&manifest, &item_id).cloned() else {
            continue;
        };
        let agent_id = agent_id.clone();
        let choices = choices.clone();
        tasks.spawn(async move {
            let report = scan_agent_cli_item(&item, &agent_id, &choices).await;
            (index, report)
        });
    }

    let mut reports = Vec::new();
    while let Some(result) = tasks.join_next().await {
        reports.push(result?);
    }
    reports.sort_by_key(|(index, _)| *index);
    Ok(reports.into_iter().map(|(_, report)| report).collect())
}

pub(crate) async fn scan_tunnel_reports(
    settings: &Value,
    choices: &StartkitChoices,
) -> anyhow::Result<Vec<StartkitItemReport>> {
    match choices.tunnel.as_str() {
        "none" => Ok(Vec::new()),
        "ngrok" => Ok(vec![StartkitItemReport {
            id: "tunnels.ngrok.sdk".to_string(),
            label: "Ngrok".to_string(),
            group: "remote".to_string(),
            category: "tunnels".to_string(),
            status: StartkitItemStatus::Ok,
            severity: None,
            version: None,
            latest_version: None,
            path: None,
            message: Some("Ngrok uses the built-in SDK".to_string()),
            actions: Vec::new(),
            manual_command: None,
            manual_url: None,
            secret: false,
            settings_key: None,
        }]),
        "localtunnel" => {
            if is_managed_mode(choices) {
                scan_startkit_item_reports(
                    settings,
                    choices,
                    &["tunnels.localtunnel.package".to_string()],
                    STARTKIT_ITEM_SCAN_TIMEOUT,
                )
                .await
            } else {
                Ok(vec![StartkitItemReport {
                    id: "tunnels.localtunnel.system".to_string(),
                    label: "localtunnel".to_string(),
                    group: "remote".to_string(),
                    category: "tunnels".to_string(),
                    status: StartkitItemStatus::Ok,
                    severity: None,
                    version: None,
                    latest_version: None,
                    path: None,
                    message: Some("System npx will be checked during setup".to_string()),
                    actions: Vec::new(),
                    manual_command: None,
                    manual_url: None,
                    secret: false,
                    settings_key: None,
                }])
            }
        }
        _ => {
            let item_id = format!("tunnels.{}.binary", choices.tunnel);
            scan_startkit_item_reports(settings, choices, &[item_id], STARTKIT_ITEM_SCAN_TIMEOUT)
                .await
        }
    }
}

async fn scan_startkit_item_reports(
    settings: &Value,
    choices: &StartkitChoices,
    item_ids: &[String],
    max_duration: Duration,
) -> anyhow::Result<Vec<StartkitItemReport>> {
    let manifest = load_manifest()?;
    let platform = current_platform().to_string();
    let paths = StartkitPaths::new(startkit_root());
    let mut tasks = JoinSet::new();

    for (index, item_id) in item_ids.iter().enumerate() {
        let Ok(item) = find_item(&manifest, item_id).cloned() else {
            continue;
        };
        let manifest = manifest.clone();
        let paths = paths.clone();
        let settings = settings.clone();
        let choices = choices.clone();
        let platform = platform.clone();
        tasks.spawn(async move {
            let report = tokio::time::timeout(
                max_duration,
                scan_item(&manifest, &paths, &item, &settings, &choices, &platform),
            )
            .await
            .unwrap_or_else(|_| StartkitItemReport {
                status: StartkitItemStatus::Error,
                message: Some("Check timed out".to_string()),
                actions: Vec::new(),
                ..base_report(&item)
            });
            (index, report)
        });
    }

    let mut reports = Vec::new();
    while let Some(result) = tasks.join_next().await {
        reports.push(result?);
    }
    reports.sort_by_key(|(index, _)| *index);
    Ok(reports.into_iter().map(|(_, report)| report).collect())
}

async fn scan_with_progress<R: Runtime>(
    app: Option<&AppHandle<R>>,
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
        if let Some(app) = app {
            emit_progress(
                app,
                None,
                item,
                StartkitItemStatus::Running,
                Some("Checking".to_string()),
                None,
            );
        }
        let report = scan_item_with_timeout(
            &manifest,
            &paths,
            item,
            settings,
            choices,
            platform,
            STARTKIT_ITEM_SCAN_TIMEOUT,
        )
        .await;
        if let Some(app) = app {
            emit_progress(
                app,
                None,
                item,
                report.status.clone(),
                report.message.clone(),
                Some(report.clone()),
            );
        }
        reports.push(report);
    }

    Ok(StartkitScanReport { plan, reports })
}

async fn scan_item_with_timeout(
    manifest: &Manifest,
    paths: &StartkitPaths,
    item: &StartkitItem,
    settings: &Value,
    choices: &StartkitChoices,
    platform: &str,
    max_duration: Duration,
) -> StartkitItemReport {
    tokio::time::timeout(
        max_duration,
        scan_item(manifest, paths, item, settings, choices, platform),
    )
    .await
    .unwrap_or_else(|_| StartkitItemReport {
        status: StartkitItemStatus::Error,
        message: Some("Check timed out".to_string()),
        actions: Vec::new(),
        ..base_report(item)
    })
}

#[allow(dead_code)]
pub async fn execute_item(
    settings: &Value,
    choices: &StartkitChoices,
    item_id: &str,
) -> anyhow::Result<StartkitItemReport> {
    execute_item_with_cancel(settings, choices, item_id, None, None).await
}

async fn execute_item_with_cancel(
    settings: &Value,
    choices: &StartkitChoices,
    item_id: &str,
    cancelled: Option<&Arc<AtomicBool>>,
    progress: StartkitProgress<'_>,
) -> anyhow::Result<StartkitItemReport> {
    let manifest = load_manifest()?;
    let platform = current_platform();
    let paths = StartkitPaths::new(startkit_root());
    let item = find_item(&manifest, item_id)?;
    if let Some(report) =
        execute_managed_toolchain_item(&manifest, item, choices, cancelled, progress).await?
    {
        return Ok(report);
    }
    if let Some(agent_id) = agent_id_from_cli_item(&item.id) {
        return execute_agent_cli_item(item, agent_id, choices, cancelled, progress).await;
    }
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

    if let Some(progress) = progress {
        progress(
            item,
            StartkitItemStatus::Running,
            Some(install_phase_message(item)),
        );
    }

    match run_script(
        &manifest,
        &paths,
        item,
        choices,
        platform,
        script_path,
        script,
        cancelled,
    )
    .await
    {
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
    run_id: String,
    skipped_item_ids: HashSet<String>,
) -> anyhow::Result<String> {
    let manifest = load_manifest()?;
    let platform = current_platform();
    let plan = plan_from_manifest(&manifest, &choices, platform)?;
    let mut had_error = false;
    let mut needs_input = false;
    let mut blocked_item_ids = HashSet::<String>::new();

    for item_id in &plan.item_ids {
        if cancelled.load(Ordering::Relaxed) {
            return Ok("cancelled".to_string());
        }

        let item = find_item(&manifest, item_id)?;
        if skipped_item_ids.contains(item_id) {
            blocked_item_ids.insert(item.id.clone());
            emit_progress(
                &app,
                Some(&run_id),
                item,
                StartkitItemStatus::Skipped,
                Some("Skipped for now".to_string()),
                Some(StartkitItemReport {
                    status: StartkitItemStatus::Skipped,
                    message: Some("Skipped for now".to_string()),
                    ..base_report(item)
                }),
            );
            continue;
        }

        if effective_item_dependencies(item, &choices, platform)
            .iter()
            .any(|dependency| blocked_item_ids.contains(*dependency))
        {
            blocked_item_ids.insert(item.id.clone());
            emit_progress(
                &app,
                Some(&run_id),
                item,
                StartkitItemStatus::Skipped,
                Some("Skipped because a dependency is not ready".to_string()),
                Some(StartkitItemReport {
                    status: StartkitItemStatus::Skipped,
                    message: Some("Skipped because a dependency is not ready".to_string()),
                    ..base_report(item)
                }),
            );
            continue;
        }

        let report = if item.kind.as_deref() == Some("builtin_channel_plugins") {
            run_channel_plugins_item(
                &app,
                Some(&run_id),
                item,
                &choices,
                &cancelled,
                &skipped_item_ids,
            )
            .await
        } else if item.kind.as_deref() == Some("managed_npm_package") {
            run_managed_npm_package_item(&app, Some(&run_id), item, &cancelled).await
        } else {
            let progress =
                |item: &StartkitItem, status: StartkitItemStatus, message: Option<String>| {
                    emit_progress(&app, Some(&run_id), item, status, message, None);
                };
            execute_item_with_cancel(
                &settings,
                &choices,
                item_id,
                Some(&cancelled),
                Some(&progress),
            )
            .await
        };

        match report {
            Ok(report) => {
                if matches!(
                    report.status,
                    StartkitItemStatus::Error | StartkitItemStatus::Blocked
                ) {
                    had_error = true;
                    blocked_item_ids.insert(item.id.clone());
                }
                if matches!(report.status, StartkitItemStatus::NeedsConfig) {
                    needs_input = true;
                }
                emit_progress(
                    &app,
                    Some(&run_id),
                    item,
                    report.status.clone(),
                    report.message.clone(),
                    Some(report),
                );
            }
            Err(err) => {
                had_error = true;
                blocked_item_ids.insert(item.id.clone());
                emit_progress(
                    &app,
                    Some(&run_id),
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

fn install_phase_message(item: &StartkitItem) -> String {
    match item.id.as_str() {
        "essentials.node" => "Downloading Node.js".to_string(),
        "tunnels.localtunnel.package" => "Installing localtunnel".to_string(),
        "tunnels.cloudflare.binary" => "Installing cloudflared".to_string(),
        _ => format!("Installing {}", item.label),
    }
}

fn emit_progress<R: Runtime>(
    app: &AppHandle<R>,
    run_id: Option<&str>,
    item: &StartkitItem,
    status: StartkitItemStatus,
    message: Option<String>,
    report: Option<StartkitItemReport>,
) {
    emit_progress_event(
        app,
        run_id,
        item.id.clone(),
        item.label.clone(),
        status,
        message,
        report,
    );
}

fn emit_progress_event<R: Runtime>(
    app: &AppHandle<R>,
    run_id: Option<&str>,
    id: String,
    label: String,
    status: StartkitItemStatus,
    message: Option<String>,
    report: Option<StartkitItemReport>,
) {
    let _ = app.emit(
        STARTKIT_PROGRESS_EVENT,
        StartkitProgressEvent {
            run_id: run_id.map(str::to_string),
            id,
            label,
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
    if let Some(report) = scan_managed_toolchain_item(item, choices, platform).await {
        return report;
    }

    if let Some(agent_id) = agent_id_from_cli_item(&item.id) {
        return scan_agent_cli_item(item, agent_id, choices).await;
    }

    if item.kind.as_deref() == Some("config") {
        return scan_config_item(item, settings);
    }

    if item.kind.as_deref() == Some("builtin_channel_plugins") {
        let ready_count = choices
            .channels
            .iter()
            .filter(|channel_id| {
                crate::onboarding::plugin_install::check_plugin_status_sync(channel_id) == "ready"
            })
            .count();
        let missing_count = choices.channels.len().saturating_sub(ready_count);
        let status = if choices.channels.is_empty() {
            StartkitItemStatus::Skipped
        } else if missing_count == 0 {
            StartkitItemStatus::Ok
        } else {
            StartkitItemStatus::Missing
        };
        return StartkitItemReport {
            status,
            message: Some(if choices.channels.is_empty() {
                "No channel plugins selected".to_string()
            } else if missing_count == 0 {
                "Channel plugins are ready".to_string()
            } else {
                format!(
                    "{missing_count} of {} channel plugins need installation",
                    choices.channels.len()
                )
            }),
            actions: if missing_count == 0 {
                Vec::new()
            } else {
                vec!["install".to_string()]
            },
            ..base_report(item)
        };
    }

    if item.kind.as_deref() == Some("managed_npm_package") {
        return scan_managed_npm_package_item(item);
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

    match run_script(
        manifest,
        paths,
        item,
        choices,
        platform,
        script_path,
        detect,
        None,
    )
    .await
    {
        Ok(output) => apply_manual_guidance(report_from_script(item, output), item, choices),
        Err(err) => StartkitItemReport {
            status: StartkitItemStatus::Error,
            message: Some(err.to_string()),
            ..base_report(item)
        },
    }
}

fn apply_manual_guidance(
    mut report: StartkitItemReport,
    item: &StartkitItem,
    choices: &StartkitChoices,
) -> StartkitItemReport {
    if !matches!(
        report.status,
        StartkitItemStatus::Missing
            | StartkitItemStatus::Outdated
            | StartkitItemStatus::Broken
            | StartkitItemStatus::Blocked
    ) {
        return report;
    }

    let Some(guidance) = manual_guidance_for_item(item, choices) else {
        return report;
    };

    report.status = StartkitItemStatus::Blocked;
    report.message = Some(guidance.message);
    report.actions = vec!["manual".to_string()];
    report.manual_command = guidance.command;
    report.manual_url = guidance.url;
    report
}

struct ManualGuidance {
    message: String,
    command: Option<String>,
    url: Option<String>,
}

fn manual_guidance_for_item(
    item: &StartkitItem,
    choices: &StartkitChoices,
) -> Option<ManualGuidance> {
    let platform = current_platform();
    match item.id.as_str() {
        "essentials.node" => Some(ManualGuidance {
            message: format!(
                "Install Node.js {} or newer. The Node.js installer includes npm. Then scan again.",
                item.min_version.as_deref().unwrap_or("22.0.0")
            ),
            command: None,
            url: Some("https://nodejs.org/en/download".to_string()),
        }),
        "essentials.git" => {
            let (command, url) = match platform {
                "macos" => (
                    Some("xcode-select --install".to_string()),
                    Some("https://developer.apple.com/documentation/xcode/installing-the-command-line-tools/".to_string()),
                ),
                "windows" => (
                    Some("winget install --id Git.Git -e --source winget".to_string()),
                    Some("https://git-scm.com/download/win".to_string()),
                ),
                _ => (None, Some("https://git-scm.com/downloads".to_string())),
            };
            Some(ManualGuidance {
                message: "Install Git, then scan again.".to_string(),
                command,
                url,
            })
        }
        "tunnels.cloudflare.binary" if !is_managed_mode(choices) => {
            let command = match platform {
                "macos" => Some("brew install cloudflared".to_string()),
                "windows" => Some(
                    "winget install --id Cloudflare.cloudflared -e --source winget".to_string(),
                ),
                _ => None,
            };
            Some(ManualGuidance {
                message: "Install cloudflared on this computer, then scan again.".to_string(),
                command,
                url: Some(
                    "https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/downloads/".to_string(),
                ),
            })
        }
        "tunnels.tailscale.binary" => Some(ManualGuidance {
            message: "Install Tailscale, sign in to your tailnet, then scan again.".to_string(),
            command: None,
            url: Some("https://tailscale.com/download".to_string()),
        }),
        _ => None,
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

fn report_from_script(item: &StartkitItem, output: ScriptOutput) -> StartkitItemReport {
    StartkitItemReport {
        status: StartkitItemStatus::from_script(&output.status),
        version: output.version,
        latest_version: output.latest_version,
        path: output.path,
        message: output.message,
        actions: output.actions,
        manual_command: output.manual_command,
        manual_url: output.manual_url,
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
        latest_version: None,
        path: None,
        message: None,
        actions: Vec::new(),
        manual_command: None,
        manual_url: None,
        secret: item.secret,
        settings_key: item.settings_key.clone(),
    }
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
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        for candidate in [
            exe_dir.join("_up_").join("resources").join("startkit"),
            exe_dir
                .join("..")
                .join("Resources")
                .join("_up_")
                .join("resources")
                .join("startkit"),
            exe_dir.join("resources").join("startkit"),
        ] {
            if candidate.exists() {
                return candidate;
            }
        }
        exe_dir.join("_up_").join("resources").join("startkit")
    }
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
        ids_for_platform(choices, "macos")
    }

    fn ids_for_platform(choices: StartkitChoices, platform: &str) -> Vec<String> {
        let manifest = load_manifest().unwrap();
        plan_from_manifest(&manifest, &choices, platform)
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
            toolchain_mode: "system".to_string(),
            portable_toolchain: Some(false),
            shell_path: false,
        });

        assert!(item_ids.contains(&"essentials.node".to_string()));
        assert!(!item_ids.contains(&"essentials.git".to_string()));
        assert!(item_ids.contains(&"agents.codex.cli".to_string()));
        assert!(!item_ids.contains(&"agents.claude.cli".to_string()));
        assert!(!item_ids.contains(&"tunnels.cloudflare.binary".to_string()));
    }

    #[test]
    fn npm_cli_agent_depends_on_node_before_cli() {
        let item_ids = ids(StartkitChoices {
            agents: vec!["claude".to_string()],
            tunnel: "none".to_string(),
            channels: Vec::new(),
            source: "global".to_string(),
            toolchain_mode: "system".to_string(),
            portable_toolchain: Some(false),
            shell_path: false,
        });

        let node = item_ids
            .iter()
            .position(|id| id == "essentials.node")
            .expect("node is planned");
        let cli = item_ids
            .iter()
            .position(|id| id == "agents.claude.cli")
            .expect("claude cli is planned");
        assert!(node < cli);
        assert_eq!(
            agent_cli_npm_install_package("claude").as_deref(),
            Some("@anthropic-ai/claude-code")
        );
    }

    #[test]
    fn npm_source_agent_depends_on_node_even_without_static_dependency() {
        let item_ids = ids(StartkitChoices {
            agents: vec!["gemini".to_string()],
            tunnel: "none".to_string(),
            channels: Vec::new(),
            source: "global".to_string(),
            toolchain_mode: "system".to_string(),
            portable_toolchain: Some(false),
            shell_path: false,
        });

        let node = item_ids
            .iter()
            .position(|id| id == "essentials.node")
            .expect("node is planned");
        let cli = item_ids
            .iter()
            .position(|id| id == "agents.gemini.cli")
            .expect("gemini cli is planned");
        assert!(node < cli);
        assert_eq!(
            agent_cli_npm_install_package("gemini").as_deref(),
            Some("@google/gemini-cli")
        );
    }

    #[test]
    fn non_npm_agent_does_not_pull_node_or_adapter() {
        let item_ids = ids(StartkitChoices {
            agents: vec!["cursor".to_string()],
            tunnel: "none".to_string(),
            channels: Vec::new(),
            source: "global".to_string(),
            toolchain_mode: "system".to_string(),
            portable_toolchain: Some(false),
            shell_path: false,
        });

        assert!(!item_ids.contains(&"essentials.node".to_string()));
        assert!(!item_ids.contains(&"essentials.git".to_string()));
        assert!(item_ids.contains(&"agents.cursor.cli".to_string()));
        assert!(agent_cli_npm_install_package("cursor").is_none());
    }

    #[test]
    fn cloudflare_plan_includes_binary_and_config_without_agents() {
        let manifest = load_manifest().unwrap();
        let item_ids = ids(StartkitChoices {
            agents: Vec::new(),
            tunnel: "cloudflare".to_string(),
            channels: Vec::new(),
            source: "cn".to_string(),
            toolchain_mode: "system".to_string(),
            portable_toolchain: Some(false),
            shell_path: false,
        });

        assert_eq!(
            item_ids,
            vec![
                "tunnels.cloudflare.binary",
                "tunnels.cloudflare.token",
                "tunnels.cloudflare.hostname"
            ]
        );
        let cloudflare = find_item(&manifest, "tunnels.cloudflare.binary").unwrap();
        assert_eq!(
            cloudflare.plugin_dependency.as_deref(),
            Some("tunnel-cloudflare")
        );
        assert!(item_uses_managed_dependency_dir(cloudflare));
        assert!(cloudflare.install.is_some());
    }

    #[test]
    fn tailscale_plan_checks_system_cli() {
        let item_ids = ids(StartkitChoices {
            agents: Vec::new(),
            tunnel: "tailscale".to_string(),
            channels: Vec::new(),
            source: "global".to_string(),
            toolchain_mode: "system".to_string(),
            portable_toolchain: Some(false),
            shell_path: false,
        });

        assert!(item_ids.contains(&"tunnels.tailscale.binary".to_string()));
        assert!(!item_ids.contains(&"tunnels.cloudflare.binary".to_string()));
    }

    #[test]
    fn managed_localtunnel_plan_installs_package_after_node() {
        let item_ids = ids(StartkitChoices {
            agents: Vec::new(),
            tunnel: "localtunnel".to_string(),
            channels: Vec::new(),
            source: "global".to_string(),
            toolchain_mode: "managed".to_string(),
            portable_toolchain: Some(false),
            shell_path: false,
        });

        let node = item_ids
            .iter()
            .position(|id| id == "essentials.node")
            .expect("node is planned");
        let package = item_ids
            .iter()
            .position(|id| id == "tunnels.localtunnel.package")
            .expect("managed localtunnel package is planned");
        assert!(node < package);
    }

    #[test]
    fn system_localtunnel_plan_only_checks_node() {
        let item_ids = ids(StartkitChoices {
            agents: Vec::new(),
            tunnel: "localtunnel".to_string(),
            channels: Vec::new(),
            source: "global".to_string(),
            toolchain_mode: "system".to_string(),
            portable_toolchain: Some(false),
            shell_path: false,
        });

        assert!(item_ids.contains(&"essentials.node".to_string()));
        assert!(!item_ids.contains(&"tunnels.localtunnel.package".to_string()));
    }

    #[test]
    fn non_npm_essentials_use_manual_guidance() {
        let manifest = load_manifest().unwrap();
        let node = find_item(&manifest, "essentials.node").unwrap();
        let report = apply_manual_guidance(
            StartkitItemReport {
                status: StartkitItemStatus::Missing,
                message: Some("Node.js will be installed".to_string()),
                actions: vec!["install".to_string()],
                ..base_report(node)
            },
            node,
            &StartkitChoices::default(),
        );

        assert_eq!(report.status, StartkitItemStatus::Blocked);
        assert_eq!(report.actions, vec!["manual".to_string()]);
        assert_eq!(
            report.manual_url.as_deref(),
            Some("https://nodejs.org/en/download")
        );
    }

    #[test]
    fn cloudflare_manual_guidance_only_applies_to_system_mode() {
        let manifest = load_manifest().unwrap();
        let cloudflare = find_item(&manifest, "tunnels.cloudflare.binary").unwrap();

        let system = manual_guidance_for_item(
            cloudflare,
            &StartkitChoices {
                tunnel: "cloudflare".to_string(),
                toolchain_mode: "system".to_string(),
                ..StartkitChoices::default()
            },
        );
        assert!(system.is_some());

        let managed = manual_guidance_for_item(
            cloudflare,
            &StartkitChoices {
                tunnel: "cloudflare".to_string(),
                toolchain_mode: "managed".to_string(),
                ..StartkitChoices::default()
            },
        );
        assert!(managed.is_none());
    }

    #[test]
    fn channels_pull_node_and_git() {
        let item_ids = ids(StartkitChoices {
            agents: Vec::new(),
            tunnel: "none".to_string(),
            channels: vec!["telegram".to_string()],
            source: "global".to_string(),
            toolchain_mode: "system".to_string(),
            portable_toolchain: Some(false),
            shell_path: false,
        });

        assert!(item_ids.contains(&"essentials.node".to_string()));
        assert!(item_ids.contains(&"essentials.git".to_string()));
        assert!(item_ids.contains(&"channels.plugins".to_string()));
    }

    #[test]
    fn portable_channels_on_macos_do_not_pull_git() {
        let item_ids = ids(StartkitChoices {
            agents: Vec::new(),
            tunnel: "none".to_string(),
            channels: vec!["telegram".to_string()],
            source: "global".to_string(),
            toolchain_mode: "managed".to_string(),
            portable_toolchain: Some(true),
            shell_path: false,
        });

        assert!(item_ids.contains(&"essentials.node".to_string()));
        assert!(!item_ids.contains(&"essentials.git".to_string()));
        assert!(item_ids.contains(&"channels.plugins".to_string()));
    }

    #[test]
    fn managed_channels_on_macos_pull_git_when_portable_disabled() {
        let item_ids = ids(StartkitChoices {
            agents: Vec::new(),
            tunnel: "none".to_string(),
            channels: vec!["telegram".to_string()],
            source: "global".to_string(),
            toolchain_mode: "managed".to_string(),
            portable_toolchain: Some(false),
            shell_path: false,
        });

        assert!(item_ids.contains(&"essentials.node".to_string()));
        assert!(item_ids.contains(&"essentials.git".to_string()));
        assert!(item_ids.contains(&"channels.plugins".to_string()));
    }

    #[test]
    fn portable_channels_on_windows_include_portable_git() {
        let item_ids = ids_for_platform(
            StartkitChoices {
                agents: Vec::new(),
                tunnel: "none".to_string(),
                channels: vec!["telegram".to_string()],
                source: "global".to_string(),
                toolchain_mode: "managed".to_string(),
                portable_toolchain: Some(true),
                shell_path: false,
            },
            "windows",
        );

        assert!(item_ids.contains(&"essentials.node".to_string()));
        assert!(item_ids.contains(&"essentials.git".to_string()));
        assert!(item_ids.contains(&"channels.plugins".to_string()));
    }

    #[test]
    fn startkit_choices_default_to_system_toolchain() {
        let choices: StartkitChoices = serde_json::from_value(serde_json::json!({
            "agents": ["codex"],
            "tunnel": "none",
            "channels": [],
            "source": "global"
        }))
        .unwrap();

        assert_eq!(choices.toolchain_mode, "system");
        assert_eq!(choices.portable_toolchain, None);
        assert!(!choices.shell_path);
    }

    #[test]
    fn shell_path_choice_no_longer_adds_environment_item() {
        let item_ids = ids(StartkitChoices {
            agents: vec!["codex".to_string()],
            tunnel: "none".to_string(),
            channels: Vec::new(),
            source: "global".to_string(),
            toolchain_mode: "system".to_string(),
            portable_toolchain: Some(false),
            shell_path: true,
        });

        assert!(!item_ids.contains(&"environment.shell_path".to_string()));
    }
}
