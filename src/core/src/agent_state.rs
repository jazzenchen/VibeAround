//! Per-agent launch state stored in `~/.vibearound/agents.json`.
//!
//! `settings.json` owns global app setup such as enabled agents. This file
//! keeps mutable Launch-tab choices and the global quick-launch default out of
//! settings so desktop, tray, and IM startup all resolve the same state.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::{auth, config, resources};

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentsPrefsFile {
    /// Launch tab's currently visible agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_agent: Option<String>,
    /// VibeAround-wide default agent used by tray quick launch and IM startup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_agent: Option<String>,
    /// Optional profile snapshot for the VibeAround-wide default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub agents: BTreeMap<String, AgentLaunchPreference>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub profile_connections: ProfileConnectionPreferences,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLaunchPreference {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileConnectionPreference {
    /// The client-side API shape the agent should use for this profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_api_type: Option<String>,
    /// Per client API shape bridge settings. The key is the selected/client
    /// API type, and `target_api_type` is the profile/provider API type.
    #[serde(default, alias = "proxy", skip_serializing_if = "BTreeMap::is_empty")]
    pub bridge: BTreeMap<String, ProfileBridgePreference>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileBridgePreference {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_api_type: Option<String>,
    // TODO(0.7.x): remove these single-model compatibility fields once all
    // saved bridge preferences have migrated to `models`.
    /// The real upstream model this bridge route should run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_model: Option<String>,
    /// Optional model id exposed to the agent. The bridge maps it back to
    /// `upstream_model` before calling the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fake_model_id: Option<String>,
    /// Optional per-route model list. Each entry can expose a fake model id to
    /// the agent while routing to a provider-specific upstream model id.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<ProfileBridgeModelPreference>,
    /// Extra provider headers for this bridge route. Catalog default headers
    /// remain owned by the provider catalog and cannot be overridden here.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileBridgeModelPreference {
    /// The real upstream model this bridge route should run for this entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_model: Option<String>,
    /// Optional model id exposed to the agent for this entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fake_model_id: Option<String>,
}

pub type ProfileConnectionPreferences =
    BTreeMap<String, BTreeMap<String, ProfileConnectionPreference>>;

pub fn read_prefs() -> AgentsPrefsFile {
    let body = match std::fs::read_to_string(prefs_path()) {
        Ok(body) => body,
        Err(_) => return AgentsPrefsFile::default(),
    };
    match serde_json::from_str(&body) {
        Ok(prefs) => prefs,
        Err(e) => {
            tracing::warn!("[launcher] agents.json parse error: {} - using default", e);
            AgentsPrefsFile::default()
        }
    }
}

pub fn resolve_selected_agent(prefs: &AgentsPrefsFile, cfg: &config::Config) -> String {
    resolve_agent_candidate(prefs.selected_agent.as_deref(), cfg)
        .or_else(|| resolve_agent_candidate(prefs.default_agent.as_deref(), cfg))
        .or_else(|| resolve_agent_candidate(Some(&cfg.default_agent), cfg))
        .or_else(|| cfg.enabled_agents.first().map(|id| canonical_agent_id(id)))
        .unwrap_or_else(|| "codex".to_string())
}

pub fn resolve_default_agent(prefs: &AgentsPrefsFile, cfg: &config::Config) -> String {
    resolve_agent_candidate(prefs.default_agent.as_deref(), cfg)
        .or_else(|| resolve_agent_candidate(prefs.selected_agent.as_deref(), cfg))
        .or_else(|| resolve_agent_candidate(Some(&cfg.default_agent), cfg))
        .or_else(|| cfg.enabled_agents.first().map(|id| canonical_agent_id(id)))
        .unwrap_or_else(|| "codex".to_string())
}

pub fn resolve_agent_profile(
    prefs: &AgentsPrefsFile,
    cfg: &config::Config,
    agent_id: &str,
) -> Option<String> {
    let agent_id = canonical_agent_id(agent_id);
    prefs
        .agents
        .get(&agent_id)
        .and_then(|preference| clean_optional_string(preference.profile_id.as_deref()))
        .or_else(|| {
            cfg.default_profiles
                .get(&agent_id)
                .and_then(|id| clean_optional_string(Some(id.as_str())))
        })
}

pub fn resolve_default_profile(
    prefs: &AgentsPrefsFile,
    cfg: &config::Config,
    agent_id: &str,
) -> Option<String> {
    let agent_id = canonical_agent_id(agent_id);
    // The app-wide default is an agent/profile pair. When the requested agent
    // is that pair's agent, the app-wide profile decision wins, including
    // `None` meaning direct launch. Other agents fall back to their own
    // per-agent default profile.
    if prefs.default_agent.is_some() && resolve_default_agent(prefs, cfg) == agent_id {
        return clean_optional_string(prefs.default_profile_id.as_deref());
    }
    resolve_agent_profile(prefs, cfg, &agent_id)
}

pub fn resolve_agent_workspace(
    prefs: &AgentsPrefsFile,
    cfg: &config::Config,
    agent_id: &str,
) -> PathBuf {
    let agent_id = canonical_agent_id(agent_id);
    prefs
        .agents
        .get(&agent_id)
        .and_then(|preference| preference.workspace.as_ref())
        .filter(|workspace| !workspace.as_os_str().is_empty())
        .cloned()
        .unwrap_or_else(|| cfg.resolve_workspace(&agent_id))
}

pub fn write_selected_agent(agent_id: &str) -> anyhow::Result<()> {
    update_prefs(|prefs| {
        freeze_legacy_default(prefs);
        prefs.selected_agent = Some(agent_id.to_string());
    })
}

pub fn write_default_launch(agent_id: &str, profile_id: Option<String>) -> anyhow::Result<()> {
    update_prefs(|prefs| {
        prefs.default_agent = Some(agent_id.to_string());
        prefs.default_profile_id = profile_id;
    })
}

pub fn write_agent_profile(agent_id: &str, profile_id: Option<String>) -> anyhow::Result<()> {
    update_prefs(|prefs| {
        freeze_legacy_default(prefs);
        let entry = prefs.agents.entry(agent_id.to_string()).or_default();
        entry.profile_id = profile_id;
        prune_empty_agent_entry(prefs, agent_id);
    })
}

pub fn write_agent_workspace(agent_id: &str, workspace: PathBuf) -> anyhow::Result<()> {
    update_prefs(|prefs| {
        let entry = prefs.agents.entry(agent_id.to_string()).or_default();
        entry.workspace = Some(workspace);
    })
}

pub fn write_profile_connection_preference(
    profile_id: &str,
    agent_id: &str,
    preference: ProfileConnectionPreference,
) -> anyhow::Result<()> {
    update_prefs(|prefs| {
        let profile_connections = prefs
            .profile_connections
            .entry(profile_id.to_string())
            .or_default();
        if connection_preference_is_empty(&preference) {
            profile_connections.remove(agent_id);
        } else {
            profile_connections.insert(agent_id.to_string(), preference);
        }
        if profile_connections.is_empty() {
            prefs.profile_connections.remove(profile_id);
        }
    })
}

pub fn remove_profile_references(profile_id: &str) -> anyhow::Result<()> {
    update_prefs(|prefs| {
        if prefs.default_profile_id.as_deref() == Some(profile_id) {
            prefs.default_profile_id = None;
        }
        for preference in prefs.agents.values_mut() {
            if preference.profile_id.as_deref() == Some(profile_id) {
                preference.profile_id = None;
            }
        }
        prefs.agents.retain(|_, preference| {
            preference.profile_id.is_some() || preference.workspace.is_some()
        });
        prefs.profile_connections.remove(profile_id);
    })
}

pub fn remove_workspace_references(workspace: &std::path::Path) -> anyhow::Result<()> {
    update_prefs(|prefs| {
        for preference in prefs.agents.values_mut() {
            if preference.workspace.as_deref() == Some(workspace) {
                preference.workspace = None;
            }
        }
        prefs.agents.retain(|_, preference| {
            preference.profile_id.is_some() || preference.workspace.is_some()
        });
    })
}

fn update_prefs(f: impl FnOnce(&mut AgentsPrefsFile)) -> anyhow::Result<()> {
    let mut prefs = read_prefs();
    f(&mut prefs);
    write_prefs(&prefs)
}

fn write_prefs(prefs: &AgentsPrefsFile) -> anyhow::Result<()> {
    let path = prefs_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {:?}", parent))?;
    }
    let body = serde_json::to_string_pretty(prefs).context("serialize agents prefs")?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, body).with_context(|| format!("write {:?}", tmp))?;
    auth::set_owner_only(&tmp).ok();
    std::fs::rename(&tmp, &path).with_context(|| format!("rename to {:?}", path))?;
    Ok(())
}

fn prefs_path() -> PathBuf {
    config::data_dir().join("agents.json")
}

fn resolve_agent_candidate(candidate: Option<&str>, cfg: &config::Config) -> Option<String> {
    let enabled = enabled_agent_ids(cfg);
    let id = candidate.map(str::trim).filter(|id| !id.is_empty())?;
    let id = canonical_agent_id(id);
    (enabled.is_empty() || enabled.contains(id.as_str())).then_some(id)
}

fn enabled_agent_ids(cfg: &config::Config) -> HashSet<&str> {
    cfg.enabled_agents.iter().map(String::as_str).collect()
}

fn canonical_agent_id(agent_id: &str) -> String {
    resources::agent_by_alias(agent_id)
        .map(|def| def.id.clone())
        .unwrap_or_else(|| agent_id.to_string())
}

fn clean_optional_string(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn freeze_legacy_default(prefs: &mut AgentsPrefsFile) {
    if prefs.default_agent.is_some() {
        return;
    }
    let Some(current) = prefs.selected_agent.as_deref() else {
        return;
    };
    let agent_id = canonical_agent_id(current);
    prefs.default_profile_id = prefs
        .agents
        .get(&agent_id)
        .and_then(|preference| preference.profile_id.clone());
    prefs.default_agent = Some(agent_id);
}

fn prune_empty_agent_entry(prefs: &mut AgentsPrefsFile, agent_id: &str) {
    let empty = prefs
        .agents
        .get(agent_id)
        .map(|entry| entry.profile_id.is_none() && entry.workspace.is_none())
        .unwrap_or(false);
    if empty {
        prefs.agents.remove(agent_id);
    }
}

fn connection_preference_is_empty(preference: &ProfileConnectionPreference) -> bool {
    preference
        .selected_api_type
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .is_empty()
        && preference.bridge.values().all(|bridge| {
            !bridge.enabled
                && bridge
                    .target_api_type
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or_default()
                    .is_empty()
                && bridge
                    .upstream_model
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or_default()
                    .is_empty()
                && bridge
                    .fake_model_id
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or_default()
                    .is_empty()
                && (!bridge.enabled || bridge.headers.is_empty())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_default_direct_overrides_agent_profile() {
        let cfg = config::Config::default();
        let prefs = AgentsPrefsFile {
            default_agent: Some("claude".to_string()),
            default_profile_id: None,
            agents: [(
                "claude".to_string(),
                AgentLaunchPreference {
                    profile_id: Some("deepseek".to_string()),
                    workspace: None,
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };

        assert_eq!(resolve_default_profile(&prefs, &cfg, "claude"), None);
    }

    #[test]
    fn switch_target_uses_agent_profile_when_not_global_default() {
        let cfg = config::Config::default();
        let prefs = AgentsPrefsFile {
            default_agent: Some("codex".to_string()),
            default_profile_id: Some("global-deepseek".to_string()),
            agents: [(
                "claude".to_string(),
                AgentLaunchPreference {
                    profile_id: Some("claude-dashscope".to_string()),
                    workspace: None,
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };

        assert_eq!(
            resolve_default_profile(&prefs, &cfg, "claude").as_deref(),
            Some("claude-dashscope")
        );
    }

    #[test]
    fn global_default_profile_overrides_same_agent_profile() {
        let cfg = config::Config::default();
        let prefs = AgentsPrefsFile {
            default_agent: Some("codex".to_string()),
            default_profile_id: Some("global-deepseek".to_string()),
            agents: [(
                "codex".to_string(),
                AgentLaunchPreference {
                    profile_id: Some("codex-small-default".to_string()),
                    workspace: None,
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };

        assert_eq!(
            resolve_default_profile(&prefs, &cfg, "codex").as_deref(),
            Some("global-deepseek")
        );
    }

    #[test]
    fn agent_workspace_overrides_builtin_default() {
        let cfg = config::Config::default();
        let workspace = PathBuf::from("/tmp/codex-project");
        let prefs = AgentsPrefsFile {
            agents: [(
                "codex".to_string(),
                AgentLaunchPreference {
                    profile_id: None,
                    workspace: Some(workspace.clone()),
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };

        assert_eq!(resolve_agent_workspace(&prefs, &cfg, "codex"), workspace);
    }

    #[test]
    fn missing_agent_workspace_uses_config_default() {
        let cfg = config::Config::default();
        let prefs = AgentsPrefsFile::default();

        assert_eq!(
            resolve_agent_workspace(&prefs, &cfg, "codex"),
            cfg.resolve_workspace("codex")
        );
    }
}
