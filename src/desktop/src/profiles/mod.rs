//! Profiles — user-managed third-party API credentials + one-click launch
//! into a system Terminal.app window with the right env vars injected.
//!
//! The schema/catalog/rendering engine lives in `common::profiles` so the
//! headless core can launch IM agents with the same profile behavior.

mod connections;
mod launcher;
mod preferences;
mod sessions;
mod store;
mod summary;
mod terminal;
mod test_connection;
mod workspace;

use std::path::PathBuf;

use common::agent_state;
use common::profiles::{normalize_legacy_profile_and_persist, schema};
use common::{config, resources};
use serde::Serialize;
use tauri::Emitter;

use self::connections::{
    profile_can_launch_agent, resolve_profile_agent_route, sanitize_profile_connection_preference,
};
use self::preferences::{launcher_preferences, validate_agent_profile_selection};
use self::sessions::list_sessions;
use self::store::{create_profile, delete_profile, get_profile, reorder_profiles, save_profile};
use self::summary::{catalog_entries, profile_summaries};
use self::workspace::{canonical_agent_id, launcher_workspace_options};
pub use common::profiles::google_oauth::GoogleOAuthStatus;
pub use common::profiles::ProfileDef;
pub use preferences::LauncherPreferences;
pub use sessions::LaunchSessionSummary;
pub(crate) use store::ordered_profiles;
pub use store::ProfileDraft;
pub use summary::{CatalogEntry, ProfileSummary};
pub use test_connection::ProfileConnectionTestResult;
pub use workspace::WorkspaceOption;

// ---------------------------------------------------------------------------
// View types — sanitized for the frontend.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutableCandidateView {
    pub path: String,
    pub realpath: Option<String>,
    pub version: Option<String>,
    pub latest_version: Option<String>,
    pub update_available: Option<bool>,
    pub source: String,
    pub source_label: String,
    pub rank: u32,
    pub selected: bool,
    pub update_command: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutableResolution {
    pub agent_id: String,
    pub configured_path: Option<String>,
    pub selected: Option<AgentExecutableCandidateView>,
    pub candidates: Vec<AgentExecutableCandidateView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutableLatestView {
    pub path: String,
    pub latest_version: Option<String>,
    pub update_available: Option<bool>,
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn profiles_list() -> Vec<ProfileSummary> {
    profile_summaries()
}

#[tauri::command]
pub fn profiles_get(id: String) -> Result<ProfileDef, String> {
    get_profile(&id)
}

#[tauri::command]
pub fn profiles_upsert(app: tauri::AppHandle, profile: ProfileDef) -> Result<(), String> {
    save_profile(&profile)?;
    emit_launch_config_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn profiles_create(app: tauri::AppHandle, draft: ProfileDraft) -> Result<ProfileDef, String> {
    let profile = create_profile(draft)?;
    emit_launch_config_changed(&app);
    Ok(profile)
}

#[tauri::command]
pub fn profiles_delete(app: tauri::AppHandle, id: String) -> Result<(), String> {
    delete_profile(&id)?;
    emit_launch_config_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn profiles_reorder(app: tauri::AppHandle, profile_ids: Vec<String>) -> Result<(), String> {
    reorder_profiles(profile_ids)?;
    emit_launch_config_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn profiles_launch(id: String, launch_target: String) -> Result<(), String> {
    profiles_launch_sync(id, launch_target)
}

pub(crate) fn profiles_launch_sync(id: String, launch_target: String) -> Result<(), String> {
    let profile = schema::load(&id)
        .map(normalize_legacy_profile_and_persist)
        .ok_or_else(|| format!("profile '{id}' not found"))?;
    if !profile_can_launch_agent(&profile, &launch_target) {
        return Err(format!("profile '{id}' cannot launch '{launch_target}'"));
    }
    launcher::launch(&profile, &launch_target).map_err(|e| e.to_string())
}

/// Launch a CLI directly with no env injection — uses whatever global
/// OAuth / login session the user already has. `agent_id` is the
/// agent registry id (e.g. "claude", "codex", "gemini", "cursor", "kiro",
/// "qwen-code", "opencode").
#[tauri::command]
pub fn profiles_launch_direct(agent_id: String) -> Result<(), String> {
    profiles_launch_direct_sync(agent_id)
}

pub(crate) fn profiles_launch_direct_sync(agent_id: String) -> Result<(), String> {
    launcher::launch_direct(&agent_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn profiles_catalog() -> Vec<CatalogEntry> {
    catalog_entries()
}

#[tauri::command]
pub fn profiles_google_oauth_status() -> GoogleOAuthStatus {
    common::profiles::google_oauth::status()
}

#[tauri::command]
pub async fn profiles_google_oauth_login() -> Result<GoogleOAuthStatus, String> {
    common::profiles::google_oauth::login_with_browser_default_client()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn profiles_test_connection(
    draft: ProfileDraft,
) -> Result<ProfileConnectionTestResult, String> {
    test_connection::test_connection(draft).await
}

// ---------------------------------------------------------------------------
// Terminal preference commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn launcher_get_preferences() -> Result<LauncherPreferences, String> {
    tauri::async_runtime::spawn_blocking(launcher_preferences)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn launcher_agent_executable_resolution(
    agent_id: String,
    scan: Option<bool>,
) -> Result<AgentExecutableResolution, String> {
    let agent_id = resources::agent_by_alias(&agent_id)
        .map(|def| def.id.clone())
        .ok_or_else(|| format!("unknown agent: '{agent_id}'"))?;
    let scan = scan.unwrap_or(false);
    let config = config::ensure_loaded();
    let availability = common::agent_availability::resolve_agent_availability(
        &agent_id,
        common::agent_availability::AgentAvailabilityRequest {
            scan_policy: if scan {
                common::agent_availability::AgentScanPolicy::Refresh
            } else {
                common::agent_availability::AgentScanPolicy::CacheOnly
            },
            toolchain_mode: config.toolchain_mode.as_str(),
            candidate_preference:
                common::agent_availability::AgentCandidatePreference::ToolchainMode,
            include_configured_version: scan,
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    let detection = availability.detection;
    let configured = availability.configured;
    let selected = availability.selected;
    let configured_path =
        agent_state::resolve_agent_executable_path(&agent_state::read_prefs(), &agent_id)
            .map(|path| path.to_string_lossy().to_string());

    let mut candidates = Vec::new();
    if let Some(candidate) = configured {
        candidates.push(candidate);
    }
    candidates.extend(detection.candidates);

    let selected_key = selected.as_ref().map(candidate_key);
    let mut candidate_views = Vec::new();
    for candidate in dedupe_agent_candidates(candidates) {
        let selected = selected_key
            .as_deref()
            .is_some_and(|key| key == candidate_key(&candidate));
        candidate_views.push(candidate_view(&agent_id, candidate, selected));
    }
    let selected = match selected {
        Some(candidate) => Some(candidate_view(&agent_id, candidate, true)),
        None => None,
    };

    Ok(AgentExecutableResolution {
        agent_id: agent_id.clone(),
        configured_path,
        selected,
        candidates: candidate_views,
    })
}

#[tauri::command]
pub async fn launcher_agent_executable_latest(
    agent_id: String,
    executable_path: String,
) -> Result<AgentExecutableLatestView, String> {
    let agent_id = resources::agent_by_alias(&agent_id)
        .map(|def| def.id.clone())
        .ok_or_else(|| format!("unknown agent: '{agent_id}'"))?;
    let path = PathBuf::from(executable_path.trim());
    if !path.is_file() {
        return Err(format!("executable path is not a file: {}", path.display()));
    }

    let detected_candidate = common::agent_detection::read_detected_agents().and_then(|detected| {
        detected
            .agents
            .get(&agent_id)
            .and_then(|detection| common::agent_detection::candidate_for_path(detection, &path))
    });
    let candidate = if let Some(candidate) = detected_candidate {
        candidate
    } else {
        common::agent_detection::manual_candidate_with_version(&agent_id, path.clone())
            .await
            .map_err(|error| error.to_string())?
    };
    let latest_version =
        common::agent_detection::latest_version_for_candidate(&agent_id, &candidate).await;
    let update_available =
        common::agent_detection::candidate_update_available(&candidate, latest_version.as_deref());

    Ok(AgentExecutableLatestView {
        path: path.to_string_lossy().to_string(),
        latest_version,
        update_available,
    })
}

#[tauri::command]
pub async fn launcher_update_agent(
    agent_id: String,
    executable_path: Option<String>,
) -> Result<(), String> {
    let agent_id = resources::agent_by_alias(&agent_id)
        .map(|def| def.id.clone())
        .ok_or_else(|| format!("unknown agent: '{agent_id}'"))?;
    let config = config::ensure_loaded();
    let availability = common::agent_availability::resolve_agent_availability(
        &agent_id,
        common::agent_availability::AgentAvailabilityRequest {
            scan_policy: common::agent_availability::AgentScanPolicy::Refresh,
            toolchain_mode: config.toolchain_mode.as_str(),
            candidate_preference:
                common::agent_availability::AgentCandidatePreference::ToolchainMode,
            include_configured_version: true,
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    let detection = availability.detection;
    let candidate = if let Some(path) = executable_path {
        let path = PathBuf::from(path.trim());
        if !path.is_file() {
            return Err(format!("executable path is not a file: {}", path.display()));
        }
        if let Some(candidate) = common::agent_detection::candidate_for_path(&detection, &path) {
            candidate
        } else {
            common::agent_detection::manual_candidate_with_version(&agent_id, path)
                .await
                .map_err(|error| error.to_string())?
        }
    } else {
        availability
            .selected
            .ok_or_else(|| format!("no selected executable for '{agent_id}'"))?
    };
    let command =
        common::agent_detection::source_command_template(&agent_id, &candidate.source, "upgrade")
            .or_else(|| {
                common::agent_detection::source_command_template(
                    &agent_id,
                    &candidate.source,
                    "install",
                )
            })
            .ok_or_else(|| format!("no update command for {}", candidate.source_label))?;

    let output = if cfg!(windows) {
        common::process::env::command("powershell.exe")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &command,
            ])
            .output()
            .await
    } else {
        common::process::env::command("sh")
            .args(["-lc", &command])
            .output()
            .await
    }
    .map_err(|error| error.to_string())?;

    if output.status.success() {
        let _ = common::agent_detection::scan_agent_and_persist(&agent_id).await;
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        Err(stderr
            .lines()
            .chain(stdout.lines())
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("agent update failed")
            .to_string())
    }
}

#[tauri::command]
pub async fn launcher_list_workspaces(
    agent_id: Option<String>,
) -> Result<Vec<WorkspaceOption>, String> {
    tauri::async_runtime::spawn_blocking(move || launcher_workspace_options(agent_id.as_deref()))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn profiles_launch_default() -> Result<(), String> {
    profiles_launch_default_sync()
}

pub(crate) fn profiles_launch_default_sync() -> Result<(), String> {
    let cfg = config::ensure_loaded();
    let agent_prefs = agent_state::read_prefs();
    let agent_id = agent_state::resolve_default_agent(&agent_prefs, &cfg);
    let profile_id = agent_state::resolve_default_profile(&agent_prefs, &cfg, &agent_id);
    if let Some(profile_id) = profile_id {
        if let Some(profile) = schema::load(&profile_id).map(normalize_legacy_profile_and_persist) {
            if profile_can_launch_agent(&profile, &agent_id) {
                return launcher::launch(&profile, &agent_id).map_err(|e| e.to_string());
            }
        }
    }
    launcher::launch_direct(&agent_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn profiles_launch_resume(
    id: String,
    launch_target: String,
    session_id: String,
) -> Result<(), String> {
    profiles_launch_resume_sync(id, launch_target, session_id)
}

pub(crate) fn profiles_launch_resume_sync(
    id: String,
    launch_target: String,
    session_id: String,
) -> Result<(), String> {
    let profile = schema::load(&id)
        .map(normalize_legacy_profile_and_persist)
        .ok_or_else(|| format!("profile '{id}' not found"))?;
    if !profile_can_launch_agent(&profile, &launch_target) {
        return Err(format!("profile '{id}' cannot launch '{launch_target}'"));
    }
    launcher::launch_resume(&profile, &launch_target, &session_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn profiles_launch_direct_resume(agent_id: String, session_id: String) -> Result<(), String> {
    profiles_launch_direct_resume_sync(agent_id, session_id)
}

pub(crate) fn profiles_launch_direct_resume_sync(
    agent_id: String,
    session_id: String,
) -> Result<(), String> {
    let agent_id = canonical_agent_id(&agent_id);
    launcher::launch_direct_resume(&agent_id, &session_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn launcher_list_sessions(
    agent_id: String,
    workspace_path: String,
    include_archived: bool,
) -> Result<Vec<LaunchSessionSummary>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        list_sessions(agent_id, workspace_path, include_archived)
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn launcher_set_default(
    app: tauri::AppHandle,
    agent_id: String,
    profile_id: Option<String>,
) -> Result<(), String> {
    let (agent_id, profile_id) = validate_agent_profile_selection(&agent_id, profile_id)?;
    agent_state::write_default_launch(&agent_id, profile_id).map_err(|e| e.to_string())?;
    emit_launch_config_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn launcher_set_agent_profile(
    app: tauri::AppHandle,
    agent_id: String,
    profile_id: Option<String>,
) -> Result<(), String> {
    let (agent_id, profile_id) = validate_agent_profile_selection(&agent_id, profile_id)?;
    agent_state::write_agent_profile(&agent_id, profile_id).map_err(|e| e.to_string())?;
    emit_launch_config_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn launcher_set_agent_launch_args(
    app: tauri::AppHandle,
    agent_id: String,
    launch_args: agent_state::AgentLaunchArgs,
) -> Result<(), String> {
    let agent_id = resources::agent_by_alias(&agent_id)
        .map(|def| def.id.clone())
        .ok_or_else(|| format!("unknown agent: '{agent_id}'"))?;
    let launch_args = sanitize_agent_launch_args(launch_args)?;
    agent_state::write_agent_launch_args(&agent_id, launch_args).map_err(|e| e.to_string())?;
    emit_launch_config_changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn launcher_set_agent_executable_path(
    app: tauri::AppHandle,
    agent_id: String,
    executable_path: Option<String>,
) -> Result<(), String> {
    let agent = resources::agent_by_alias(&agent_id)
        .cloned()
        .ok_or_else(|| format!("unknown agent: '{agent_id}'"))?;
    let agent_id = agent.id.clone();
    let executable = match executable_path {
        Some(path) => {
            let path = PathBuf::from(path.trim());
            if agent.direct_only {
                if !is_valid_direct_only_executable_path(&agent_id, &path) {
                    return Err(format!(
                        "agent path is not a valid {}: {}",
                        direct_only_executable_path_label(),
                        path.display()
                    ));
                }
                Some(direct_only_executable_preference(&agent_id, path))
            } else {
                if !path.is_file() {
                    return Err(format!("executable path is not a file: {}", path.display()));
                }
                let detected_candidate =
                    common::agent_detection::read_detected_agents().and_then(|detected| {
                        detected.agents.get(&agent_id).and_then(|detection| {
                            common::agent_detection::candidate_for_path(detection, &path)
                        })
                    });
                let candidate = if let Some(candidate) = detected_candidate {
                    candidate
                } else {
                    common::agent_detection::manual_candidate_with_version(&agent_id, path.clone())
                        .await
                        .map_err(|e| e.to_string())?
                };
                Some(
                    common::agent_detection::executable_preference_from_candidate_path(
                        &candidate, &path,
                    ),
                )
            }
        }
        None => None,
    };
    agent_state::write_agent_executable(&agent_id, executable).map_err(|e| e.to_string())?;
    emit_launch_config_changed(&app);
    Ok(())
}

fn is_valid_direct_only_executable_path(agent_id: &str, path: &std::path::Path) -> bool {
    if cfg!(target_os = "macos") {
        path.is_dir() || path.is_file()
    } else if cfg!(target_os = "windows") {
        path.is_file() || matches_detected_windows_start_app(agent_id, path)
    } else {
        path.is_file()
    }
}

fn direct_only_executable_path_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "app bundle or executable"
    } else if cfg!(target_os = "windows") {
        "executable file or Start app ID"
    } else {
        "executable file"
    }
}

fn matches_detected_windows_start_app(agent_id: &str, path: &std::path::Path) -> bool {
    if !cfg!(target_os = "windows") {
        return false;
    }
    let Some(detected) = crate::desktop_detection::read_detected_desktop_apps() else {
        return false;
    };
    detected
        .apps
        .get(agent_id)
        .and_then(|detection| detection.entry.as_ref())
        .is_some_and(|entry| {
            entry.source == "windows_start_apps"
                && path.to_string_lossy().eq_ignore_ascii_case(&entry.path)
        })
}

fn direct_only_executable_preference(
    agent_id: &str,
    path: PathBuf,
) -> agent_state::AgentExecutablePreference {
    let realpath = path.canonicalize().ok();
    let mut preference = agent_state::AgentExecutablePreference::manual(path.clone());
    preference.realpath = realpath;

    let Some(detected) = crate::desktop_detection::read_detected_desktop_apps() else {
        return preference;
    };
    let Some(entry) = detected
        .apps
        .get(agent_id)
        .and_then(|detection| detection.entry.as_ref())
    else {
        return preference;
    };
    let detected_path = PathBuf::from(&entry.path);
    let detected_realpath = detected_path.canonicalize().ok();
    if path == detected_path
        || preference
            .realpath
            .as_ref()
            .zip(detected_realpath.as_ref())
            .is_some_and(|(left, right)| left == right)
    {
        preference.source = entry.source.clone();
        preference.source_label = entry.source_label.clone();
    }
    preference
}

#[tauri::command]
pub fn launcher_set_selected_agent(app: tauri::AppHandle, agent_id: String) -> Result<(), String> {
    let agent_id = resources::agent_by_alias(&agent_id)
        .map(|def| def.id.clone())
        .ok_or_else(|| format!("unknown agent: '{agent_id}'"))?;
    agent_state::write_selected_agent(&agent_id).map_err(|e| e.to_string())?;
    emit_launch_config_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn launcher_set_terminal(terminal_id: String) -> Result<(), String> {
    let choice = terminal::TerminalChoice::from_id(&terminal_id)
        .ok_or_else(|| format!("unknown terminal: '{}'", terminal_id))?;
    if !terminal::TerminalChoice::ALL.contains(&choice) {
        return Err(format!(
            "terminal '{}' is not supported on this platform",
            terminal_id
        ));
    }
    terminal::write_preference(choice).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn launcher_set_workspace(
    app: tauri::AppHandle,
    workspace_path: String,
    agent_id: Option<String>,
) -> Result<(), String> {
    workspace::set_workspace(&workspace_path, agent_id)?;
    emit_launch_config_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn launcher_remove_workspace(
    app: tauri::AppHandle,
    workspace_path: String,
) -> Result<(), String> {
    workspace::remove_workspace(workspace_path)?;
    emit_launch_config_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn launcher_reorder_workspaces(
    app: tauri::AppHandle,
    workspace_paths: Vec<String>,
) -> Result<(), String> {
    workspace::reorder_workspaces(workspace_paths)?;
    emit_launch_config_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn launcher_set_compatibility_bridge(
    app: tauri::AppHandle,
    mode: String,
) -> Result<(), String> {
    let mode = terminal::CompatibilityBridgeMode::from_id(&mode)
        .ok_or_else(|| format!("unknown compatibility bridge mode: '{mode}'"))?;
    terminal::write_compatibility_bridge_preference(mode).map_err(|e| e.to_string())?;
    emit_launch_config_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn launcher_set_local_agent_api_enabled(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<(), String> {
    config::update_settings_json(|root| {
        if !root.is_object() {
            *root = serde_json::json!({});
        }
        let Some(root_obj) = root.as_object_mut() else {
            return;
        };
        let entry = root_obj
            .entry("local_agent_api".to_string())
            .or_insert_with(|| serde_json::json!({}));
        if !entry.is_object() {
            *entry = serde_json::json!({});
        }
        if let Some(settings) = entry.as_object_mut() {
            settings.insert("enabled".to_string(), serde_json::json!(enabled));
        }
    })?;
    emit_launch_config_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn launcher_set_profile_connection(
    app: tauri::AppHandle,
    profile_id: String,
    agent_id: String,
    preference: agent_state::ProfileConnectionPreference,
) -> Result<(), String> {
    let agent_id = validate_connection_agent_id(agent_id)?;
    let mut profile = schema::load(&profile_id)
        .map(normalize_legacy_profile_and_persist)
        .ok_or_else(|| format!("profile '{profile_id}' not found"))?;
    let preference = sanitize_profile_connection_preference(&profile, &agent_id, preference)?;

    schema::set_connection(&mut profile, &agent_id, preference);
    schema::save(&profile).map_err(|e| e.to_string())?;
    emit_launch_config_changed(&app);
    Ok(())
}

fn validate_connection_agent_id(agent_id: String) -> Result<String, String> {
    match agent_id.as_str() {
        "claude" | "claude-desktop" | "codex" | "codex-desktop" | "gemini" | "opencode" | "pi" => {
            Ok(agent_id)
        }
        other => Err(format!("unsupported connection target: '{other}'")),
    }
}

fn sanitize_agent_launch_args(
    launch_args: agent_state::AgentLaunchArgs,
) -> Result<agent_state::AgentLaunchArgs, String> {
    Ok(agent_state::AgentLaunchArgs {
        terminal: sanitize_arg_list("terminal", launch_args.terminal)?,
        acp: sanitize_arg_list("acp", launch_args.acp)?,
    })
}

fn sanitize_arg_list(kind: &str, args: Vec<String>) -> Result<Vec<String>, String> {
    if args.len() > 64 {
        return Err(format!("{kind} launch args cannot exceed 64 entries"));
    }

    let mut out = Vec::with_capacity(args.len());
    for arg in args {
        let arg = arg.trim().to_string();
        if arg.is_empty() {
            continue;
        }
        if arg.chars().any(|ch| ch == '\0' || ch == '\n' || ch == '\r') {
            return Err(format!("{kind} launch args cannot contain line breaks"));
        }
        out.push(arg);
    }
    Ok(out)
}

fn dedupe_agent_candidates(
    candidates: Vec<common::agent_detection::AgentCandidate>,
) -> Vec<common::agent_detection::AgentCandidate> {
    let mut seen = std::collections::BTreeSet::new();
    let mut deduped = Vec::new();
    for candidate in candidates {
        if seen.insert(candidate_key(&candidate)) {
            deduped.push(candidate);
        }
    }
    deduped
}

fn candidate_key(candidate: &common::agent_detection::AgentCandidate) -> String {
    candidate
        .realpath
        .clone()
        .unwrap_or_else(|| candidate.path.clone())
}

fn candidate_view(
    agent_id: &str,
    candidate: common::agent_detection::AgentCandidate,
    selected: bool,
) -> AgentExecutableCandidateView {
    let update_command =
        common::agent_detection::source_command_template(agent_id, &candidate.source, "upgrade")
            .or_else(|| {
                common::agent_detection::source_command_template(
                    agent_id,
                    &candidate.source,
                    "install",
                )
            });
    AgentExecutableCandidateView {
        path: candidate.path,
        realpath: candidate.realpath,
        version: candidate.version,
        latest_version: None,
        update_available: None,
        update_command,
        source: candidate.source,
        source_label: candidate.source_label,
        rank: candidate.rank,
        selected,
    }
}

pub(super) fn resolve_launch_workspace(agent_id: &str) -> anyhow::Result<PathBuf> {
    workspace::resolve_launch_workspace(agent_id)
}

fn emit_launch_config_changed(app: &tauri::AppHandle) {
    let _ = app.emit(crate::tray::LAUNCH_CONFIG_CHANGED_EVENT, ());
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use super::profiles_launch_direct;
    use super::{sanitize_agent_launch_args, validate_connection_agent_id};
    use common::agent_state::AgentLaunchArgs;
    #[cfg(windows)]
    use std::path::PathBuf;

    #[test]
    fn accepts_supported_profile_connection_targets() {
        for agent_id in [
            "claude",
            "claude-desktop",
            "codex",
            "codex-desktop",
            "gemini",
            "opencode",
            "pi",
        ] {
            assert_eq!(
                validate_connection_agent_id(agent_id.to_string()).unwrap(),
                agent_id
            );
        }
    }

    #[test]
    fn rejects_unknown_profile_connection_target() {
        assert!(validate_connection_agent_id("cursor".to_string()).is_err());
    }

    #[test]
    fn cleans_agent_launch_args() {
        let args = sanitize_agent_launch_args(AgentLaunchArgs {
            terminal: vec![
                "".to_string(),
                "  --sandbox ".to_string(),
                " danger-full-access ".to_string(),
            ],
            acp: Vec::new(),
        })
        .unwrap();

        assert_eq!(
            args.terminal,
            vec!["--sandbox".to_string(), "danger-full-access".to_string()]
        );
    }

    #[test]
    fn rejects_multiline_agent_launch_args() {
        assert!(sanitize_agent_launch_args(AgentLaunchArgs {
            terminal: vec!["--flag\nvalue".to_string()],
            acp: Vec::new(),
        })
        .is_err());
    }

    #[cfg(windows)]
    #[test]
    fn launch_direct_invokes_va_launch_boundary() {
        let _guard = env_test_lock().lock().expect("env test lock");
        let dir = unique_test_dir("direct-va-launch");
        let data_dir = dir.join("data");
        let workspace = dir.join("workspace");
        std::fs::create_dir_all(&data_dir).expect("create data dir");
        std::fs::create_dir_all(&workspace).expect("create workspace dir");
        std::fs::write(
            data_dir.join("launcher.json"),
            serde_json::json!({
                "terminal": "powershell",
                "workspace": workspace.to_string_lossy()
            })
            .to_string(),
        )
        .expect("write launcher prefs");

        let fake_bin = dir.join("va-launch.cmd");
        let capture = dir.join("capture.json");
        std::fs::write(
            &fake_bin,
            "@echo off\r\nif \"%~1\"==\"--profile-path\" copy /Y \"%~2\" \"%VA_LAUNCH_CAPTURE%\" >nul & exit /b 0\r\nexit /b 2\r\n",
        )
        .expect("write fake va-launch");

        let previous_data_dir = std::env::var_os("VIBEAROUND_DATA_DIR");
        let previous_bin = std::env::var_os("VIBEAROUND_VA_LAUNCH_BIN");
        let previous_capture = std::env::var_os("VA_LAUNCH_CAPTURE");
        std::env::set_var("VIBEAROUND_DATA_DIR", &data_dir);
        std::env::set_var("VIBEAROUND_VA_LAUNCH_BIN", &fake_bin);
        std::env::set_var("VA_LAUNCH_CAPTURE", &capture);
        let _ = common::config::reload();

        profiles_launch_direct("codex".to_string()).expect("direct launch calls va-launch");

        let captured = read_to_string_eventually(&capture);
        let value: serde_json::Value =
            serde_json::from_str(&captured).expect("parse captured profile");

        restore_env("VIBEAROUND_DATA_DIR", previous_data_dir);
        restore_env("VIBEAROUND_VA_LAUNCH_BIN", previous_bin);
        restore_env("VA_LAUNCH_CAPTURE", previous_capture);
        let _ = common::config::reload();
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(value["agent"], "codex");
        assert_eq!(value["command"], "codex");
        assert_eq!(value["terminal"], "powershell");
        assert_eq!(value["workspace"], workspace.to_string_lossy().to_string());
    }

    #[cfg(windows)]
    fn unique_test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "vibearound-profiles-{name}-{}",
            uuid::Uuid::new_v4()
        ))
    }

    #[cfg(windows)]
    fn restore_env(key: &str, previous: Option<std::ffi::OsString>) {
        match previous {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

    #[cfg(windows)]
    fn read_to_string_eventually(path: &std::path::Path) -> String {
        for _ in 0..50 {
            if let Ok(body) = std::fs::read_to_string(path) {
                if body.trim().is_empty() {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    continue;
                }
                return body;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        std::fs::read_to_string(path).expect("read captured profile")
    }

    #[cfg(windows)]
    fn env_test_lock() -> &'static std::sync::Mutex<()> {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
    }
}
