use serde_json::Value;

use super::WebSocketSpec;
use crate::http::AuthRequirement;
use crate::runtime::{AgentRuntime, ChannelRuntime, TunnelRuntime};
use crate::sessions::SessionListItem;

pub fn channels_ws() -> WebSocketSpec {
    WebSocketSpec::new("/ws/channels", AuthRequirement::BearerToken)
}

pub fn tunnels_ws() -> WebSocketSpec {
    WebSocketSpec::new("/ws/tunnels", AuthRequirement::BearerToken)
}

pub fn agents_runtime_ws() -> WebSocketSpec {
    WebSocketSpec::new("/ws/agents/runtime", AuthRequirement::BearerToken)
}

pub fn sessions_ws() -> WebSocketSpec {
    WebSocketSpec::new("/ws/sessions", AuthRequirement::BearerToken)
}

pub fn decode_channels_event(value: Value) -> crate::Result<Vec<ChannelRuntime>> {
    serde_json::from_value(value).map_err(crate::ClientError::Decode)
}

pub fn decode_tunnels_event(value: Value) -> crate::Result<Vec<TunnelRuntime>> {
    serde_json::from_value(value).map_err(crate::ClientError::Decode)
}

pub fn decode_agents_runtime_event(value: Value) -> crate::Result<Vec<AgentRuntime>> {
    serde_json::from_value(value).map_err(crate::ClientError::Decode)
}

pub fn decode_sessions_event(value: Value) -> crate::Result<Vec<SessionListItem>> {
    serde_json::from_value(value).map_err(crate::ClientError::Decode)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn decodes_domain_snapshot_events() {
        let channels = decode_channels_event(json!([{
            "kind": "feishu",
            "version": "1.0.0",
            "plugin_dir": null,
            "status": "running",
            "reason": null
        }]))
        .expect("channels");
        assert_eq!(channels[0].kind, "feishu");

        let tunnels = decode_tunnels_event(json!([{
            "provider": "cloudflare",
            "url": "https://example.test",
            "status": { "state": "running" },
            "uptime_secs": 12
        }]))
        .expect("tunnels");
        assert_eq!(tunnels[0].provider, "cloudflare");

        let agents = decode_agents_runtime_event(json!([{
            "route_key": "workspace-thread",
            "channel_kind": "workspace",
            "chat_id": "workspace-thread",
            "attached_routes": [],
            "cli_kind": "codex",
            "profile": null,
            "profile_label": null,
            "session_id": null,
            "workspace": "/tmp/project",
            "busy": false,
            "failed": null,
            "started_at": 0,
            "agent_name": "Codex",
            "agent_title": null,
            "agent_version": "1.0.0",
            "multi_agent_turns": [],
            "subagents": []
        }]))
        .expect("agents");
        assert_eq!(agents[0].channel_kind, "workspace");

        let sessions = decode_sessions_event(json!([{
            "session_id": "session-1",
            "tool": "codex",
            "status": { "type": "running", "tool": "codex" },
            "created_at": 1,
            "project_path": "/tmp/project",
            "profile_id": "default",
            "profile_label": "Default",
            "launch_target": "codex",
            "tmux_session": null
        }]))
        .expect("sessions");
        assert_eq!(sessions[0].session_id, "session-1");
    }
}
