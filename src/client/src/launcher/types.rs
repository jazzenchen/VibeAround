use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherPreferencesResponse {
    pub selected_agent: String,
    pub default_agent: String,
    pub default_profile_id: Option<String>,
    pub enabled_agents: Vec<String>,
    pub agent_preferences: BTreeMap<String, LauncherAgentPreferenceSummary>,
    pub local_agent_api_enabled: bool,
    pub profile_connections: Value,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherAgentPreferenceSummary {
    pub profile_id: Option<String>,
    pub workspace: Option<String>,
    pub executable_path: Option<String>,
    #[serde(default)]
    pub launch_args: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfileBody<'a> {
    pub agent_id: &'a str,
    pub profile_id: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentWorkspaceBody<'a> {
    pub agent_id: &'a str,
    pub workspace: &'a str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLaunchArgsBody<'a> {
    pub agent_id: &'a str,
    pub launch_args: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedAgentBody<'a> {
    pub agent_id: &'a str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EnabledBody {
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileConnectionBody<'a> {
    pub profile_id: &'a str,
    pub agent_id: &'a str,
    pub preference: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchPlanBody<'a> {
    pub agent_id: Option<&'a str>,
    pub profile_id: Option<&'a str>,
    pub launch_target: Option<&'a str>,
    pub session_id: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchPlanResponse {
    pub launch_id: String,
    pub agent_id: String,
    pub profile_id: Option<String>,
    pub launch_target: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<LaunchPlanEnvVar>,
    pub cwd: String,
    pub resume_session_id: Option<String>,
    pub native_execution: bool,
    pub display: LaunchPlanDisplay,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LaunchPlanEnvVar {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LaunchPlanDisplay {
    pub title: String,
}
