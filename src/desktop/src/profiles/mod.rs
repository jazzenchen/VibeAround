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
mod workspace;

use std::path::PathBuf;

use common::agent_state;
use common::profiles::{normalize_legacy_profile_and_persist, schema};
use common::{config, resources};
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
pub use workspace::WorkspaceOption;

// ---------------------------------------------------------------------------
// View types — sanitized for the frontend.
// ---------------------------------------------------------------------------

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
/// agents.json id (e.g. "claude", "codex", "gemini", "cursor", "kiro",
/// "qwen-code", "opencode").
#[tauri::command]
pub fn profiles_launch_direct(agent_id: String) -> Result<(), String> {
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
pub async fn launcher_list_workspaces(
    agent_id: Option<String>,
) -> Result<Vec<WorkspaceOption>, String> {
    tauri::async_runtime::spawn_blocking(move || launcher_workspace_options(agent_id.as_deref()))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn profiles_launch_default() -> Result<(), String> {
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
pub fn launcher_set_profile_connection(
    app: tauri::AppHandle,
    profile_id: String,
    agent_id: String,
    preference: agent_state::ProfileConnectionPreference,
) -> Result<(), String> {
    let agent_id = validate_connection_agent_id(agent_id)?;
    let profile = schema::load(&profile_id)
        .map(normalize_legacy_profile_and_persist)
        .ok_or_else(|| format!("profile '{profile_id}' not found"))?;
    let preference = sanitize_profile_connection_preference(&profile, &agent_id, preference)?;

    agent_state::write_profile_connection_preference(&profile.id, &agent_id, preference)
        .map_err(|e| e.to_string())?;
    emit_launch_config_changed(&app);
    Ok(())
}

fn validate_connection_agent_id(agent_id: String) -> Result<String, String> {
    match agent_id.as_str() {
        "claude" | "codex" | "gemini" | "opencode" | "pi" => Ok(agent_id),
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

pub(super) fn resolve_launch_workspace(agent_id: &str) -> anyhow::Result<PathBuf> {
    workspace::resolve_launch_workspace(agent_id)
}

fn emit_launch_config_changed(app: &tauri::AppHandle) {
    let _ = app.emit(crate::tray::LAUNCH_CONFIG_CHANGED_EVENT, ());
}

#[cfg(test)]
mod tests {
    use super::{sanitize_agent_launch_args, validate_connection_agent_id};
    use common::agent_state::AgentLaunchArgs;

    #[test]
    fn accepts_supported_profile_connection_targets() {
        for agent_id in ["claude", "codex", "gemini", "opencode", "pi"] {
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
}
