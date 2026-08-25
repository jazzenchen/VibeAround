use crate::http::{join_path, AuthRequirement, HttpMethod, RequestSpec, ResponseSpec};
use crate::Result;

use super::{AgentRuntime, AgentsConfig, ChannelRuntime, TunnelRuntime};

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

pub fn shutdown_thread_host(thread_id: &str) -> RequestSpec {
    RequestSpec::new(
        HttpMethod::Post,
        format!(
            "{}/shutdown-host",
            join_path("/api/workspace-threads", thread_id)
        ),
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
    use crate::runtime::TunnelStatus;

    #[test]
    fn shutdown_thread_host_encodes_thread_id() {
        let request = shutdown_thread_host("wt/thread-1");
        assert_eq!(request.method, HttpMethod::Post);
        assert_eq!(
            request.path,
            "/api/workspace-threads/wt%2Fthread-1/shutdown-host"
        );
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
