//! Launcher preference summary and validation.

use std::collections::{BTreeMap, HashSet};

use common::agent_state;
use common::profiles::{normalize_legacy_profile_and_persist, schema};
use common::{config, resources};
use serde::Serialize;

use super::connections::{merged_profile_connections, profile_can_launch_agent};
use super::terminal;
use super::workspace::{canonical_agent_id, resolve_agent_workspace_preference, WorkspaceOption};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOption {
    pub id: String,
    pub label: String,
    pub installed: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherPreferences {
    /// `id` of the currently-preferred terminal.
    pub terminal: String,
    /// Every supported terminal, with an `installed` flag the UI uses to gray
    /// out unavailable choices instead of just hiding them.
    pub options: Vec<TerminalOption>,
    /// Resolved cwd used for profile/direct launches.
    pub workspace: String,
    /// Suggested cwd choices surfaced in the Launch header.
    pub workspace_options: Vec<WorkspaceOption>,
    /// Canonical agent id selected in the Launch tab.
    pub selected_agent: String,
    /// Per-agent launch choices stored in `~/.vibearound/agents.json`.
    pub agent_preferences: BTreeMap<String, AgentLaunchPreferenceSummary>,
    /// VibeAround-wide default agent for tray quick launch and IM startup.
    pub default_agent: String,
    /// Optional profile paired with the VibeAround-wide default agent.
    pub default_profile_id: Option<String>,
    /// Agent ids enabled by onboarding/settings.json.
    pub enabled_agents: Vec<String>,
    /// Back-compat alias for older UI code. New writes go to agents.json.
    pub default_profiles: BTreeMap<String, String>,
    /// Global policy for wrapping OpenAI-compatible profile launches through
    /// VibeAround's local compatibility bridge.
    pub compatibility_bridge: terminal::CompatibilityBridgeMode,
    /// Per-profile connection choices for launch targets that can run via
    /// the local API bridge.
    pub profile_connections: agent_state::ProfileConnectionPreferences,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLaunchPreferenceSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    #[serde(skip_serializing_if = "agent_state::AgentLaunchArgs::is_empty")]
    pub launch_args: agent_state::AgentLaunchArgs,
}

pub(super) fn launcher_preferences() -> LauncherPreferences {
    let installed_ids: HashSet<&'static str> = terminal::detect_installed()
        .iter()
        .map(|c| c.id())
        .collect();
    let options = terminal::TerminalChoice::ALL
        .iter()
        .map(|c| TerminalOption {
            id: c.id().to_string(),
            label: c.label().to_string(),
            installed: installed_ids.contains(c.id()),
        })
        .collect();
    let cfg = config::ensure_loaded();
    let agent_prefs = agent_state::read_prefs();
    let selected_agent = agent_state::resolve_selected_agent(&agent_prefs, &cfg);
    let default_agent = agent_state::resolve_default_agent(&agent_prefs, &cfg);
    let default_profile_id =
        agent_state::resolve_default_profile(&agent_prefs, &cfg, &default_agent);
    let workspace = resolve_agent_workspace_preference(&selected_agent, &agent_prefs)
        .unwrap_or_else(|_| terminal::launch_home_dir().unwrap_or_else(|_| config::data_dir()))
        .to_string_lossy()
        .to_string();
    let agent_preferences = summarize_agent_preferences(&agent_prefs, &cfg);
    let default_profiles = agent_preferences
        .iter()
        .filter_map(|(agent_id, preference)| {
            preference
                .profile_id
                .as_ref()
                .map(|profile_id| (agent_id.clone(), profile_id.clone()))
        })
        .collect();

    LauncherPreferences {
        terminal: terminal::read_preference().id().to_string(),
        options,
        workspace,
        workspace_options: Vec::new(),
        selected_agent,
        agent_preferences,
        default_agent,
        default_profile_id,
        enabled_agents: cfg.enabled_agents.clone(),
        default_profiles,
        compatibility_bridge: terminal::read_compatibility_bridge_preference(),
        profile_connections: merged_profile_connections(&agent_prefs),
    }
}

pub(super) fn validate_agent_profile_selection(
    agent_id: &str,
    profile_id: Option<String>,
) -> Result<(String, Option<String>), String> {
    let agent_id = resources::agent_by_alias(agent_id)
        .map(|def| def.id.clone())
        .ok_or_else(|| format!("unknown agent: '{agent_id}'"))?;
    let profile_id = profile_id
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty());

    if let Some(profile_id) = &profile_id {
        let profile = schema::load(profile_id)
            .map(normalize_legacy_profile_and_persist)
            .ok_or_else(|| format!("profile '{profile_id}' not found"))?;
        if !profile_can_launch_agent(&profile, &agent_id) {
            return Err(format!("profile '{profile_id}' cannot launch '{agent_id}'"));
        }
    }

    Ok((agent_id, profile_id))
}

fn summarize_agent_preferences(
    agent_prefs: &agent_state::AgentsPrefsFile,
    cfg: &config::Config,
) -> BTreeMap<String, AgentLaunchPreferenceSummary> {
    let mut agent_ids: HashSet<String> = cfg
        .enabled_agents
        .iter()
        .map(|id| canonical_agent_id(id))
        .collect();
    agent_ids.extend(agent_prefs.agents.keys().map(|id| canonical_agent_id(id)));
    agent_ids.extend(cfg.default_profiles.keys().map(|id| canonical_agent_id(id)));

    let mut out = BTreeMap::new();
    for agent_id in agent_ids {
        let stored = agent_prefs.agents.get(&agent_id);
        let profile_id = stored
            .and_then(|preference| preference.profile_id.clone())
            .or_else(|| cfg.default_profiles.get(&agent_id).cloned());
        let workspace = stored
            .and_then(|preference| preference.workspace.as_ref())
            .map(|path| path.to_string_lossy().to_string());
        let launch_args = stored
            .map(|preference| preference.launch_args.clone())
            .unwrap_or_default();
        if profile_id.is_some() || workspace.is_some() || !launch_args.is_empty() {
            out.insert(
                agent_id,
                AgentLaunchPreferenceSummary {
                    profile_id,
                    workspace,
                    launch_args,
                },
            );
        }
    }
    out
}
