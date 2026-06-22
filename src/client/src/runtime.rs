use serde::Deserialize;
use serde_json::Value;

use crate::http::{join_path, AuthRequirement, HttpMethod, RequestSpec, ResponseSpec};
use crate::Result;

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

pub fn agents() -> RequestSpec {
    RequestSpec::new(HttpMethod::Get, "/api/agents", AuthRequirement::BearerToken)
}

pub fn decode_agents(response: ResponseSpec) -> Result<AgentsConfig> {
    response.decode()
}

pub fn channels() -> RequestSpec {
    RequestSpec::new(
        HttpMethod::Get,
        "/api/channels",
        AuthRequirement::BearerToken,
    )
}

pub fn decode_channels(response: ResponseSpec) -> Result<Vec<ChannelRuntime>> {
    response.decode()
}

pub fn sync_channels() -> RequestSpec {
    RequestSpec::new(
        HttpMethod::Post,
        "/api/channels/sync",
        AuthRequirement::BearerToken,
    )
}

pub fn reload_settings() -> RequestSpec {
    RequestSpec::new(
        HttpMethod::Post,
        "/api/settings/reload",
        AuthRequirement::BearerToken,
    )
}

pub fn tunnels() -> RequestSpec {
    RequestSpec::new(
        HttpMethod::Get,
        "/api/tunnels",
        AuthRequirement::BearerToken,
    )
}

pub fn decode_tunnels(response: ResponseSpec) -> Result<Vec<TunnelRuntime>> {
    response.decode()
}

pub fn agents_runtime() -> RequestSpec {
    RequestSpec::new(
        HttpMethod::Get,
        "/api/agents/runtime",
        AuthRequirement::BearerToken,
    )
}

pub fn decode_agents_runtime(response: ResponseSpec) -> Result<Vec<AgentRuntime>> {
    response.decode()
}

pub fn stop_channel(kind: &str) -> RequestSpec {
    RequestSpec::new(
        HttpMethod::Post,
        format!("{}/stop", join_path("/api/channels", kind)),
        AuthRequirement::BearerToken,
    )
}

pub fn restart_channel(kind: &str) -> RequestSpec {
    RequestSpec::new(
        HttpMethod::Post,
        format!("{}/restart", join_path("/api/channels", kind)),
        AuthRequirement::BearerToken,
    )
}

pub fn start_channel(kind: &str) -> RequestSpec {
    RequestSpec::new(
        HttpMethod::Post,
        format!("{}/start", join_path("/api/channels", kind)),
        AuthRequirement::BearerToken,
    )
}

pub fn kill_tunnel(provider: &str) -> RequestSpec {
    RequestSpec::new(
        HttpMethod::Delete,
        join_path("/api/tunnels", provider),
        AuthRequirement::BearerToken,
    )
}

pub fn kill_agent(route_key: &str) -> RequestSpec {
    RequestSpec::new(
        HttpMethod::Delete,
        join_path("/api/agents", route_key),
        AuthRequirement::BearerToken,
    )
}

pub fn kill_pty(session_id: &str) -> RequestSpec {
    RequestSpec::new(
        HttpMethod::Delete,
        join_path("/api/pty", session_id),
        AuthRequirement::BearerToken,
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn kill_agent_encodes_route_key() {
        let request = kill_agent("telegram:chat/1");
        assert_eq!(request.method, HttpMethod::Delete);
        assert_eq!(request.path, "/api/agents/telegram%3Achat%2F1");
    }

    #[test]
    fn decodes_tunnel_status() {
        let response = ResponseSpec::json(
            200,
            json!([
                {
                    "provider": "cloudflare",
                    "url": null,
                    "status": { "state": "failed", "error": "boom" },
                    "uptime_secs": 3
                }
            ]),
        );
        let tunnels = decode_tunnels(response).expect("decode");
        assert!(matches!(
            tunnels[0].status,
            TunnelStatus::Failed { ref error } if error == "boom"
        ));
    }
}
