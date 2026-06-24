use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AgentsConfig {
    pub agents: Vec<AgentInfo>,
    pub default_agent: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ChannelRuntime {
    pub kind: String,
    pub version: Option<String>,
    pub plugin_dir: Option<String>,
    pub status: ChannelStatus,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelStatus {
    NotStarted,
    Spawning,
    Running,
    Crashed,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TunnelRuntime {
    pub provider: String,
    pub url: Option<String>,
    pub status: TunnelStatus,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TunnelStatus {
    Running,
    Stopped { reason: String },
    Failed { error: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AgentAttachedRoute {
    pub route_key: String,
    pub channel_kind: String,
    pub chat_id: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AgentRuntime {
    pub route_key: String,
    pub channel_kind: String,
    pub chat_id: String,
    #[serde(default)]
    pub attached_routes: Vec<AgentAttachedRoute>,
    pub cli_kind: Option<String>,
    pub profile: Option<String>,
    pub profile_label: Option<String>,
    pub session_id: Option<String>,
    pub workspace: Option<String>,
    pub busy: bool,
    pub failed: Option<String>,
    pub started_at: u64,
    pub agent_name: Option<String>,
    pub agent_title: Option<String>,
    pub agent_version: Option<String>,
    pub multi_agent_turns: Vec<Value>,
    pub subagents: Vec<Value>,
}
