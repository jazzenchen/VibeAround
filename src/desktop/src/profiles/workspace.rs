//! Workspace choices for profile/direct launches.

use std::path::{Path, PathBuf};

use common::{agent_state, config, resources};
use serde::Serialize;

use super::terminal;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceOption {
    pub path: String,
    pub label: String,
    pub detail: String,
    pub kind: String,
    pub is_default: bool,
}

pub(super) fn launcher_workspace_options(agent_id: Option<&str>) -> Vec<WorkspaceOption> {
    let builtin = config::builtin_workspaces_dir();
    let home = terminal::launch_home_dir().unwrap_or_else(|_| config::data_dir());
    let (cfg, agent_prefs) = agent_state::read_config_and_prefs();
    let selected = agent_id
        .map(canonical_agent_id)
        .and_then(|agent_id| resolve_agent_workspace_preference(&agent_id, &agent_prefs).ok())
        .or_else(|| terminal::resolve_workspace_preference().ok());
    let mut out = Vec::new();
    push_workspace_option(&mut out, &home, "Home", "home", false);
    for workspace in cfg.all_workspaces() {
        let is_default = config::workspace_paths_equal(&workspace, &builtin);
        let kind = if is_default { "built-in" } else { "workspace" };
        let label = if is_default {
            "Default workspace".to_string()
        } else {
            path_label(&workspace)
        };
        push_workspace_option(&mut out, &workspace, &label, kind, is_default);
    }
    if let Some(path) = selected {
        if !out
            .iter()
            .any(|option| config::workspace_paths_equal(Path::new(&option.path), &path))
        {
            let label = path_label(&path);
            push_workspace_option(&mut out, &path, &label, "selected", false);
        }
    }
    out
}

pub(super) fn set_workspace(workspace_path: &str, agent_id: Option<String>) -> Result<(), String> {
    let path =
        terminal::canonical_workspace_path(Path::new(workspace_path)).map_err(|e| e.to_string())?;
    let should_register = should_register_launcher_workspace(&path);
    let (cfg, agent_prefs) = agent_state::read_config_and_prefs();
    let agent_id = agent_id
        .map(|id| canonical_agent_id(&id))
        .unwrap_or_else(|| agent_state::resolve_selected_agent(&agent_prefs, &cfg));
    if should_register {
        agent_state::write_registered_agent_workspace(&agent_id, path)
    } else {
        agent_state::write_agent_workspace(&agent_id, path)
    }
    .map_err(|e| e.to_string())
}

pub(super) fn remove_workspace(workspace_path: String) -> Result<(), String> {
    let path = PathBuf::from(workspace_path);
    config::remove_registered_workspace(&path).map_err(|error| error.to_string())
}

pub(super) fn reorder_workspaces(workspace_paths: Vec<String>) -> Result<(), String> {
    let requested = workspace_paths
        .into_iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    config::reorder_workspace_paths(&requested)
}

pub(super) fn canonical_agent_id(agent_id: &str) -> String {
    resources::agent_by_alias(agent_id)
        .map(|def| def.id.clone())
        .unwrap_or_else(|| agent_id.to_string())
}

pub(super) fn resolve_agent_workspace_preference(
    agent_id: &str,
    agent_prefs: &agent_state::AgentsPrefsFile,
) -> anyhow::Result<PathBuf> {
    if let Some(workspace) = agent_prefs
        .agents
        .get(agent_id)
        .and_then(|preference| preference.workspace.as_ref())
    {
        return terminal::canonical_workspace_path(workspace);
    }
    terminal::resolve_workspace_preference()
}

pub(super) fn resolve_launch_workspace(agent_id: &str) -> anyhow::Result<PathBuf> {
    let agent_prefs = agent_state::read_prefs();
    resolve_agent_workspace_preference(agent_id, &agent_prefs)
}

fn should_register_launcher_workspace(path: &Path) -> bool {
    let builtin = config::builtin_workspaces_dir();
    if config::workspace_paths_equal(path, &builtin) {
        return false;
    }
    if terminal::launch_home_dir()
        .as_ref()
        .map(|home| config::workspace_paths_equal(path, home))
        .unwrap_or(false)
    {
        return false;
    }
    true
}

fn push_workspace_option(
    out: &mut Vec<WorkspaceOption>,
    path: &Path,
    label: &str,
    kind: &str,
    is_default: bool,
) {
    if out
        .iter()
        .any(|option| config::workspace_paths_equal(Path::new(&option.path), path))
    {
        return;
    }
    out.push(WorkspaceOption {
        path: path.to_string_lossy().to_string(),
        label: label.to_string(),
        detail: path.to_string_lossy().to_string(),
        kind: kind.to_string(),
        is_default,
    });
}

fn path_label(path: &Path) -> String {
    if let Some(name) = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
    {
        name.to_string()
    } else {
        path.to_string_lossy().to_string()
    }
}
