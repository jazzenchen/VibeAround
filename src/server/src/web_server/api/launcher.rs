//! Launcher management and plan APIs.

use std::collections::BTreeMap;

use axum::{http::StatusCode, Json};
use common::agent_state;
use common::profiles::{connections, normalize_legacy_profile_and_persist, runtime, schema};
use common::{config, resources};
use serde::Deserialize;

const LOCAL_BRIDGE_NO_PROXY: &str = "localhost,127.0.0.1,::1,0.0.0.0,127.0.0.0/8";
const LOCAL_BRIDGE_PROXY_ENV_KEYS: &[&str] = &[
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
];
const VIBEAROUND_LAUNCH_ID_ENV: &str = "VIBEAROUND_LAUNCH_ID";
const VIBEAROUND_LAUNCH_TARGET_ENV: &str = "VIBEAROUND_LAUNCH_TARGET";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfileBody {
    pub agent_id: String,
    pub profile_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentWorkspaceBody {
    pub agent_id: String,
    pub workspace: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLaunchArgsBody {
    pub agent_id: String,
    pub launch_args: agent_state::AgentLaunchArgs,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedAgentBody {
    pub agent_id: String,
}

#[derive(Debug, Deserialize)]
pub struct EnabledBody {
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileConnectionBody {
    pub profile_id: String,
    pub agent_id: String,
    pub preference: agent_state::ProfileConnectionPreference,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchPlanBody {
    pub agent_id: Option<String>,
    pub profile_id: Option<String>,
    pub launch_target: Option<String>,
    pub session_id: Option<String>,
}

/// GET /api/launcher/preferences -- server-owned launcher runtime preferences.
pub async fn get_launcher_preferences_handler(
) -> Json<crate::api_types::LauncherPreferencesResponse> {
    Json(launcher_preferences())
}

/// PUT /api/launcher/default-agent -- set app-wide default agent/profile.
pub async fn set_default_launch_handler(
    Json(body): Json<AgentProfileBody>,
) -> Result<Json<crate::api_types::LauncherPreferencesResponse>, (StatusCode, String)> {
    let (agent_id, profile_id) = validate_agent_profile_selection(&body.agent_id, body.profile_id)?;
    agent_state::write_default_launch(&agent_id, profile_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(launcher_preferences()))
}

/// PUT /api/launcher/agent-profile -- set one agent's default profile.
pub async fn set_agent_profile_handler(
    Json(body): Json<AgentProfileBody>,
) -> Result<Json<crate::api_types::LauncherPreferencesResponse>, (StatusCode, String)> {
    let (agent_id, profile_id) = validate_agent_profile_selection(&body.agent_id, body.profile_id)?;
    agent_state::write_agent_profile(&agent_id, profile_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(launcher_preferences()))
}

/// PUT /api/launcher/agent-workspace -- set one agent's default workspace.
pub async fn set_agent_workspace_handler(
    Json(body): Json<AgentWorkspaceBody>,
) -> Result<Json<crate::api_types::LauncherPreferencesResponse>, (StatusCode, String)> {
    let (agent_id, workspace) =
        validate_agent_workspace_selection(&body.agent_id, &body.workspace)?;
    agent_state::write_agent_workspace(&agent_id, workspace)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(launcher_preferences()))
}

/// PUT /api/launcher/agent-launch-args -- set terminal/acp args for one agent.
pub async fn set_agent_launch_args_handler(
    Json(body): Json<AgentLaunchArgsBody>,
) -> Result<Json<crate::api_types::LauncherPreferencesResponse>, (StatusCode, String)> {
    let agent_id = canonical_agent_id(&body.agent_id)?;
    let launch_args = sanitize_agent_launch_args(body.launch_args)?;
    agent_state::write_agent_launch_args(&agent_id, launch_args)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(launcher_preferences()))
}

/// PUT /api/launcher/selected-agent -- set the launch tab's selected agent.
pub async fn set_selected_agent_handler(
    Json(body): Json<SelectedAgentBody>,
) -> Result<Json<crate::api_types::LauncherPreferencesResponse>, (StatusCode, String)> {
    let agent_id = canonical_agent_id(&body.agent_id)?;
    agent_state::write_selected_agent(&agent_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(launcher_preferences()))
}

/// PUT /api/launcher/local-agent-api -- enable/disable sessionless local API.
pub async fn set_local_agent_api_handler(
    Json(body): Json<EnabledBody>,
) -> Result<Json<crate::api_types::LauncherPreferencesResponse>, (StatusCode, String)> {
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
            settings.insert("enabled".to_string(), serde_json::json!(body.enabled));
        }
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(launcher_preferences()))
}

/// PUT /api/launcher/profile-connection -- set profile-to-agent bridge routing.
pub async fn set_profile_connection_handler(
    Json(body): Json<ProfileConnectionBody>,
) -> Result<Json<crate::api_types::LauncherPreferencesResponse>, (StatusCode, String)> {
    let agent_id = canonical_agent_id(&body.agent_id)?;
    let mut profile = load_profile(&body.profile_id)?;
    let preference =
        connections::sanitize_profile_connection_preference(&profile, &agent_id, body.preference)
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    schema::set_connection(&mut profile, &agent_id, preference);
    schema::save(&profile).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(launcher_preferences()))
}

/// POST /api/launcher/plan -- build a launch plan without executing it.
pub async fn launcher_plan_handler(
    Json(body): Json<LaunchPlanBody>,
) -> Result<Json<crate::api_types::LaunchPlanResponse>, (StatusCode, String)> {
    build_launch_plan(body).map(Json)
}

fn launcher_preferences() -> crate::api_types::LauncherPreferencesResponse {
    let cfg = config::ensure_loaded();
    let prefs = agent_state::read_prefs();
    let selected_agent = agent_state::resolve_selected_agent(&prefs, &cfg);
    let default_agent = agent_state::resolve_default_agent(&prefs, &cfg);
    let default_profile_id = agent_state::resolve_default_profile(&prefs, &cfg, &default_agent);
    let agent_preferences = summarize_agent_preferences(&prefs);

    crate::api_types::LauncherPreferencesResponse {
        selected_agent,
        default_agent,
        default_profile_id,
        enabled_agents: cfg.enabled_agents.clone(),
        agent_preferences,
        local_agent_api_enabled: cfg.local_agent_api.enabled,
        profile_connections: connections::merged_profile_connections(),
    }
}

fn summarize_agent_preferences(
    prefs: &agent_state::AgentsPrefsFile,
) -> BTreeMap<String, crate::api_types::LauncherAgentPreferenceSummary> {
    prefs
        .agents
        .iter()
        .map(|(agent_id, preference)| {
            let executable_path = agent_state::resolve_agent_executable(prefs, agent_id)
                .map(|executable| executable.path.to_string_lossy().to_string());
            (
                agent_id.clone(),
                crate::api_types::LauncherAgentPreferenceSummary {
                    profile_id: preference.profile_id.clone(),
                    workspace: preference
                        .workspace
                        .as_ref()
                        .map(|path| path.to_string_lossy().to_string()),
                    executable_path,
                    launch_args: preference.launch_args.clone(),
                },
            )
        })
        .collect()
}

fn validate_agent_profile_selection(
    agent_id: &str,
    profile_id: Option<String>,
) -> Result<(String, Option<String>), (StatusCode, String)> {
    let agent_id = canonical_agent_id(agent_id)?;
    let profile_id = profile_id
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty());

    if let Some(profile_id) = &profile_id {
        let profile = load_profile(profile_id)?;
        if !connections::profile_can_launch_agent(&profile, &agent_id) {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("profile '{profile_id}' cannot launch '{agent_id}'"),
            ));
        }
    }

    Ok((agent_id, profile_id))
}

fn validate_agent_workspace_selection(
    agent_id: &str,
    workspace: &str,
) -> Result<(String, std::path::PathBuf), (StatusCode, String)> {
    let agent_id = canonical_agent_id(agent_id)?;
    let workspace = common::workspace::normalize_workspace_cwd(std::path::PathBuf::from(workspace));
    if !workspace.exists() || !workspace.is_dir() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "workspace does not exist or is not a directory: {}",
                workspace.to_string_lossy()
            ),
        ));
    }
    Ok((agent_id, workspace))
}

fn canonical_agent_id(agent_id: &str) -> Result<String, (StatusCode, String)> {
    resources::agent_by_alias(agent_id)
        .map(|def| def.id.clone())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                format!("unknown agent: '{agent_id}'"),
            )
        })
}

fn load_profile(id: &str) -> Result<schema::ProfileDef, (StatusCode, String)> {
    schema::load(id)
        .map(normalize_legacy_profile_and_persist)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("profile '{id}' not found")))
}

fn sanitize_agent_launch_args(
    launch_args: agent_state::AgentLaunchArgs,
) -> Result<agent_state::AgentLaunchArgs, (StatusCode, String)> {
    fn clean(args: Vec<String>) -> Vec<String> {
        args.into_iter()
            .map(|arg| arg.trim().to_string())
            .filter(|arg| !arg.is_empty())
            .collect()
    }

    Ok(agent_state::AgentLaunchArgs {
        terminal: clean(launch_args.terminal),
        acp: clean(launch_args.acp),
    })
}

pub(crate) fn build_launch_plan(
    body: LaunchPlanBody,
) -> Result<crate::api_types::LaunchPlanResponse, (StatusCode, String)> {
    let launch_id = uuid::Uuid::new_v4().to_string();
    match body.profile_id.clone() {
        Some(profile_id) => build_profile_launch_plan(&launch_id, profile_id, body),
        None => build_direct_launch_plan(&launch_id, body),
    }
}

fn build_direct_launch_plan(
    launch_id: &str,
    body: LaunchPlanBody,
) -> Result<crate::api_types::LaunchPlanResponse, (StatusCode, String)> {
    let cfg = config::ensure_loaded();
    let prefs = agent_state::read_prefs();
    let agent_id = match body.agent_id {
        Some(agent_id) => canonical_agent_id(&agent_id)?,
        None => agent_state::resolve_selected_agent(&prefs, &cfg),
    };
    let agent = resources::agent_by_id(&agent_id).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            format!("unknown agent: '{agent_id}'"),
        )
    })?;
    let workspace = agent_state::resolve_agent_workspace(&prefs, &cfg, &agent_id);
    let (command, resume_args) = if let Some(session_id) = body.session_id.as_deref() {
        resume_command_for_agent(&agent_id, session_id)?
    } else {
        (
            agent_state::resolve_agent_executable_path(&prefs, &agent_id)
                .map(|path| path.to_string_lossy().to_string())
                .unwrap_or_else(|| agent.pty_command_for_current_platform().to_string()),
            Vec::new(),
        )
    };
    let mut args = agent_state::resolve_agent_terminal_args(&prefs, &agent_id);
    args.extend(resume_args);

    Ok(crate::api_types::LaunchPlanResponse {
        launch_id: launch_id.to_string(),
        agent_id: agent_id.clone(),
        profile_id: None,
        launch_target: body.launch_target.unwrap_or_else(|| agent_id.clone()),
        command,
        args,
        env: Vec::new(),
        cwd: workspace.to_string_lossy().to_string(),
        resume_session_id: body.session_id,
        native_execution: agent.direct_only,
        display: crate::api_types::LaunchPlanDisplay {
            title: format!("{} (direct)", agent.display_name),
        },
    })
}

fn build_profile_launch_plan(
    launch_id: &str,
    profile_id: String,
    body: LaunchPlanBody,
) -> Result<crate::api_types::LaunchPlanResponse, (StatusCode, String)> {
    let cfg = config::ensure_loaded();
    let prefs = agent_state::read_prefs();
    let profile = load_profile(&profile_id)?;
    let launch_target = body.launch_target.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "launchTarget is required for profile launch plans".to_string(),
        )
    })?;
    let agent_id = runtime::agent_id_for(&launch_target)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
        .to_string();
    let agent = resources::agent_by_id(&agent_id).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            format!("unknown agent: '{agent_id}'"),
        )
    })?;
    let route =
        connections::resolve_profile_agent_route(&profile, &launch_target).ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                format!("profile '{}' cannot launch '{}'", profile.id, launch_target),
            )
        })?;
    let rendered = runtime::render_for_agent_route(&profile, &launch_target, launch_id, &route)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let mut env = runtime::materialize_env(&profile.id, rendered.clone())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if route.bridge_target_api_type.is_some() {
        append_local_bridge_proxy_bypass_env(&mut env);
    } else {
        runtime::append_settings_proxy_env(&profile, &mut env)
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    }
    append_vibearound_launch_context_env(&mut env, &profile.id, &launch_target, launch_id);

    let workspace = agent_state::resolve_agent_workspace(&prefs, &cfg, &agent_id);
    let (command, resume_args) = if let Some(session_id) = body.session_id.as_deref() {
        resume_command_for_agent(&agent_id, session_id)?
    } else {
        (
            agent_state::resolve_agent_executable_path(&prefs, &agent_id)
                .map(|path| path.to_string_lossy().to_string())
                .unwrap_or_else(|| agent.pty_command_for_current_platform().to_string()),
            Vec::new(),
        )
    };
    let mut args = rendered.command_args;
    args.extend(agent_state::resolve_agent_terminal_args(&prefs, &agent_id));
    args.extend(resume_args);

    Ok(crate::api_types::LaunchPlanResponse {
        launch_id: launch_id.to_string(),
        agent_id,
        profile_id: Some(profile.id),
        launch_target,
        command,
        args,
        env: env
            .into_iter()
            .map(|(key, value)| crate::api_types::LaunchPlanEnvVar { key, value })
            .collect(),
        cwd: workspace.to_string_lossy().to_string(),
        resume_session_id: body.session_id,
        native_execution: agent.direct_only,
        display: crate::api_types::LaunchPlanDisplay {
            title: profile.label,
        },
    })
}

fn append_vibearound_launch_context_env(
    env: &mut Vec<(String, String)>,
    profile_id: &str,
    launch_target: &str,
    launch_id: &str,
) {
    env.retain(|(key, _)| {
        key != VIBEAROUND_LAUNCH_ID_ENV
            && key != common::agent::launch::VIBEAROUND_PROFILE_ID_ENV
            && key != VIBEAROUND_LAUNCH_TARGET_ENV
    });
    env.push((VIBEAROUND_LAUNCH_ID_ENV.to_string(), launch_id.to_string()));
    env.push((
        common::agent::launch::VIBEAROUND_PROFILE_ID_ENV.to_string(),
        profile_id.to_string(),
    ));
    env.push((
        VIBEAROUND_LAUNCH_TARGET_ENV.to_string(),
        launch_target.to_string(),
    ));
}

fn append_local_bridge_proxy_bypass_env(env: &mut Vec<(String, String)>) {
    env.retain(|(key, _)| !LOCAL_BRIDGE_PROXY_ENV_KEYS.contains(&key.as_str()));
    env.extend([
        ("HTTP_PROXY".to_string(), String::new()),
        ("HTTPS_PROXY".to_string(), String::new()),
        ("ALL_PROXY".to_string(), String::new()),
        ("http_proxy".to_string(), String::new()),
        ("https_proxy".to_string(), String::new()),
        ("all_proxy".to_string(), String::new()),
        ("NO_PROXY".to_string(), LOCAL_BRIDGE_NO_PROXY.to_string()),
        ("no_proxy".to_string(), LOCAL_BRIDGE_NO_PROXY.to_string()),
    ]);
}

fn resume_command_for_agent(
    agent_id: &str,
    session_id: &str,
) -> Result<(String, Vec<String>), (StatusCode, String)> {
    let command = match agent_id {
        "claude" => (
            "claude".to_string(),
            vec![
                "--resume".to_string(),
                session_id.to_string(),
                "--permission-mode".to_string(),
                "acceptEdits".to_string(),
            ],
        ),
        "codex" => (
            "codex".to_string(),
            vec!["resume".to_string(), session_id.to_string()],
        ),
        "pi" => (
            "pi".to_string(),
            vec!["--session".to_string(), session_id.to_string()],
        ),
        "gemini" => (
            "gemini".to_string(),
            vec!["--resume".to_string(), session_id.to_string()],
        ),
        "opencode" => (
            "opencode".to_string(),
            vec!["--session".to_string(), session_id.to_string()],
        ),
        "cursor" => (
            "cursor-agent".to_string(),
            vec!["--resume".to_string(), session_id.to_string()],
        ),
        "qwen-code" => (
            "qwen".to_string(),
            vec!["--resume".to_string(), session_id.to_string()],
        ),
        other => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("resume launch is not supported for agent '{other}'"),
            ))
        }
    };
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_agent_workspace_selection() {
        let path =
            std::env::temp_dir().join(format!("va-launcher-workspace-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).expect("create temp workspace");

        let (agent_id, workspace) =
            validate_agent_workspace_selection("codex", path.to_str().expect("utf8 path"))
                .expect("valid workspace");

        assert_eq!(agent_id, "codex");
        assert_eq!(
            workspace,
            std::fs::canonicalize(&path).expect("canonical path")
        );

        std::fs::remove_dir_all(&path).expect("remove temp workspace");
        let error = validate_agent_workspace_selection("codex", path.to_str().expect("utf8 path"))
            .expect_err("missing workspace");
        assert_eq!(error.0, StatusCode::BAD_REQUEST);
    }
}
