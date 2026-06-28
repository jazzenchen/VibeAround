//! Per-agent launch preferences stored under `settings.json.launcher`.
//!
//! `settings.json` owns both the enabled-agent list and the mutable Launch-tab
//! choices so desktop, tray, server, and IM startup resolve the same state from
//! one durable config file.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::profiles::catalog::ContentCapabilities;
use crate::{config, resources};

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
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
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentLaunchPreference {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable: Option<AgentExecutablePreference>,
    #[serde(default, skip_serializing_if = "AgentLaunchArgs::is_empty")]
    pub launch_args: AgentLaunchArgs,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct AgentExecutablePreference {
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub realpath: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default = "manual_executable_source")]
    pub source: String,
    #[serde(default = "manual_executable_source_label")]
    pub source_label: String,
    #[serde(default)]
    pub rank: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentLaunchArgs {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terminal: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acp: Vec<String>,
}

impl AgentLaunchArgs {
    pub fn is_empty(&self) -> bool {
        self.terminal.is_empty() && self.acp.is_empty()
    }
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
    /// Optional manual input capability overrides for custom or newly released
    /// upstream models that are not yet represented in the provider catalog.
    #[serde(default, skip_serializing_if = "ContentCapabilities::is_empty")]
    pub capabilities: ContentCapabilities,
}

pub type ProfileConnectionPreferences =
    BTreeMap<String, BTreeMap<String, ProfileConnectionPreference>>;

pub fn read_prefs() -> AgentsPrefsFile {
    config::read_settings_json()
        .ok()
        .and_then(|root| root.get("launcher").cloned())
        .and_then(
            |launcher| match serde_json::from_value::<AgentsPrefsFile>(launcher) {
                Ok(prefs) => Some(prefs),
                Err(error) => {
                    tracing::warn!(
                        "[launcher] settings.json launcher prefs parse error: {} - using default",
                        error
                    );
                    None
                }
            },
        )
        .unwrap_or_default()
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
        .or_else(|| resolve_agent_candidate(Some(&cfg.default_agent), cfg))
        .or_else(|| cfg.enabled_agents.first().map(|id| canonical_agent_id(id)))
        .unwrap_or_else(|| "codex".to_string())
}

pub fn resolve_agent_profile(
    prefs: &AgentsPrefsFile,
    _cfg: &config::Config,
    agent_id: &str,
) -> Option<String> {
    let agent_id = canonical_agent_id(agent_id);
    prefs
        .agents
        .get(&agent_id)
        .and_then(|preference| clean_optional_string(preference.profile_id.as_deref()))
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

pub fn resolve_agent_executable_path(prefs: &AgentsPrefsFile, agent_id: &str) -> Option<PathBuf> {
    resolve_agent_executable(prefs, agent_id).map(|executable| executable.path)
}

pub fn resolve_agent_executable(
    prefs: &AgentsPrefsFile,
    agent_id: &str,
) -> Option<AgentExecutablePreference> {
    let agent_id = canonical_agent_id(agent_id);
    prefs.agents.get(&agent_id).and_then(|preference| {
        preference
            .executable
            .clone()
            .filter(|executable| !executable.path.as_os_str().is_empty())
    })
}

pub fn resolve_agent_terminal_args(prefs: &AgentsPrefsFile, agent_id: &str) -> Vec<String> {
    let agent_id = canonical_agent_id(agent_id);
    prefs
        .agents
        .get(&agent_id)
        .map(|preference| preference.launch_args.terminal.clone())
        .unwrap_or_default()
}

pub fn resolve_agent_acp_args(prefs: &AgentsPrefsFile, agent_id: &str) -> Vec<String> {
    let agent_id = canonical_agent_id(agent_id);
    prefs
        .agents
        .get(&agent_id)
        .map(|preference| preference.launch_args.acp.clone())
        .unwrap_or_default()
}

pub fn write_selected_agent(agent_id: &str) -> anyhow::Result<()> {
    update_prefs(|prefs| {
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

pub fn write_agent_executable_path(
    agent_id: &str,
    executable_path: Option<PathBuf>,
) -> anyhow::Result<()> {
    write_agent_executable(
        agent_id,
        executable_path.map(AgentExecutablePreference::manual),
    )
}

pub fn write_agent_executable(
    agent_id: &str,
    executable: Option<AgentExecutablePreference>,
) -> anyhow::Result<()> {
    update_prefs(|prefs| {
        let entry = prefs.agents.entry(agent_id.to_string()).or_default();
        entry.executable = executable;
        prune_empty_agent_entry(prefs, agent_id);
    })
}

pub fn write_agent_launch_args(agent_id: &str, launch_args: AgentLaunchArgs) -> anyhow::Result<()> {
    update_prefs(|prefs| {
        let entry = prefs.agents.entry(agent_id.to_string()).or_default();
        entry.launch_args = launch_args;
        prune_empty_agent_entry(prefs, agent_id);
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
            preference.profile_id.is_some()
                || preference.workspace.is_some()
                || preference.executable.is_some()
                || !preference.launch_args.is_empty()
        });
    })
}

pub fn remove_workspace_references(workspace: &std::path::Path) -> anyhow::Result<()> {
    update_prefs(|prefs| {
        for preference in prefs.agents.values_mut() {
            if preference
                .workspace
                .as_deref()
                .map(|candidate| paths_equal(candidate, workspace))
                .unwrap_or(false)
            {
                preference.workspace = None;
            }
        }
        prefs.agents.retain(|_, preference| {
            preference.profile_id.is_some()
                || preference.workspace.is_some()
                || preference.executable.is_some()
                || !preference.launch_args.is_empty()
        });
    })
}

fn paths_equal(left: &std::path::Path, right: &std::path::Path) -> bool {
    left == right
        || std::fs::canonicalize(left)
            .ok()
            .zip(std::fs::canonicalize(right).ok())
            .map(|(left, right)| left == right)
            .unwrap_or(false)
}

fn update_prefs(f: impl FnOnce(&mut AgentsPrefsFile)) -> anyhow::Result<()> {
    let mut prefs = read_prefs();
    f(&mut prefs);
    write_prefs(&prefs)
}

fn write_prefs(prefs: &AgentsPrefsFile) -> anyhow::Result<()> {
    let value = serde_json::to_value(prefs)?;
    let prefs_obj = value.as_object().cloned().unwrap_or_default();
    config::update_settings_json(|root| {
        if !root.is_object() {
            *root = Value::Object(Map::new());
        }
        let Some(root_obj) = root.as_object_mut() else {
            return;
        };
        let launcher = root_obj
            .entry("launcher".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if !launcher.is_object() {
            *launcher = Value::Object(Map::new());
        }
        let Some(launcher_obj) = launcher.as_object_mut() else {
            return;
        };
        merge_pref_field(launcher_obj, &prefs_obj, "selected_agent");
        merge_pref_field(launcher_obj, &prefs_obj, "default_agent");
        merge_pref_field(launcher_obj, &prefs_obj, "default_profile_id");
        merge_pref_field(launcher_obj, &prefs_obj, "agents");
        if launcher_obj.is_empty() {
            root_obj.remove("launcher");
        }
    })
    .map_err(anyhow::Error::msg)
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

fn prune_empty_agent_entry(prefs: &mut AgentsPrefsFile, agent_id: &str) {
    let empty = prefs
        .agents
        .get(agent_id)
        .map(|entry| {
            entry.profile_id.is_none()
                && entry.workspace.is_none()
                && entry.executable.is_none()
                && entry.launch_args.is_empty()
        })
        .unwrap_or(false);
    if empty {
        prefs.agents.remove(agent_id);
    }
}

fn merge_pref_field(launcher: &mut Map<String, Value>, prefs: &Map<String, Value>, key: &str) {
    if let Some(value) = prefs.get(key) {
        launcher.insert(key.to_string(), value.clone());
    } else {
        launcher.remove(key);
    }
}

impl AgentExecutablePreference {
    pub fn manual(path: PathBuf) -> Self {
        Self {
            path,
            realpath: None,
            version: None,
            source: manual_executable_source(),
            source_label: manual_executable_source_label(),
            rank: 0,
            package: None,
        }
    }
}

fn manual_executable_source() -> String {
    "manual_path".to_string()
}

fn manual_executable_source_label() -> String {
    "Manual path".to_string()
}

pub(crate) fn connection_preference_is_empty(preference: &ProfileConnectionPreference) -> bool {
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
                && bridge.models.is_empty()
                && bridge.headers.is_empty()
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
                    ..Default::default()
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
                    ..Default::default()
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
                    ..Default::default()
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
                    ..Default::default()
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

    #[test]
    fn structured_executable_preference_resolves_by_alias() {
        let prefs = AgentsPrefsFile {
            agents: [(
                "codex".to_string(),
                AgentLaunchPreference {
                    executable: Some(AgentExecutablePreference {
                        path: PathBuf::from("/opt/homebrew/bin/codex"),
                        realpath: Some(PathBuf::from(
                            "/opt/homebrew/lib/node_modules/@openai/codex/bin/codex.js",
                        )),
                        version: Some("codex-cli 0.139.0".to_string()),
                        source: "npm_global".to_string(),
                        source_label: "npm global (Homebrew prefix)".to_string(),
                        rank: 1,
                        package: Some("@openai/codex".to_string()),
                    }),
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };

        let executable = resolve_agent_executable(&prefs, "openai-codex").unwrap();
        assert_eq!(executable.path, PathBuf::from("/opt/homebrew/bin/codex"));
        assert_eq!(executable.source, "npm_global");
        assert_eq!(
            resolve_agent_executable_path(&prefs, "codex").unwrap(),
            PathBuf::from("/opt/homebrew/bin/codex")
        );
    }

    #[test]
    fn resolves_agent_launch_args_by_alias() {
        let prefs = AgentsPrefsFile {
            agents: [(
                "codex".to_string(),
                AgentLaunchPreference {
                    launch_args: AgentLaunchArgs {
                        terminal: vec!["--sandbox".to_string(), "danger-full-access".to_string()],
                        acp: vec!["--strict-config".to_string()],
                    },
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };

        assert_eq!(
            resolve_agent_terminal_args(&prefs, "openai-codex"),
            vec!["--sandbox".to_string(), "danger-full-access".to_string()]
        );
        assert_eq!(
            resolve_agent_acp_args(&prefs, "codex"),
            vec!["--strict-config".to_string()]
        );
    }

    #[test]
    fn connection_preference_with_model_routes_is_not_empty() {
        let preference = ProfileConnectionPreference {
            selected_api_type: None,
            bridge: [(
                "anthropic".to_string(),
                ProfileBridgePreference {
                    enabled: false,
                    models: vec![ProfileBridgeModelPreference {
                        upstream_model: Some("deepseek-v4-pro".to_string()),
                        fake_model_id: Some("claude-sonnet-4-5".to_string()),
                        capabilities: Default::default(),
                    }],
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
        };

        assert!(!connection_preference_is_empty(&preference));
    }
}
