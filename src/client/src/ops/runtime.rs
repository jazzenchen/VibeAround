use crate::operation::Operation;
use crate::runtime::{AgentRuntime, AgentsConfig, ChannelRuntime, TunnelRuntime};

use super::decode_success;

pub fn runtime_agents() -> Operation<AgentsConfig> {
    Operation::new(crate::runtime::agents(), crate::runtime::decode_agents)
}

pub fn runtime_channels() -> Operation<Vec<ChannelRuntime>> {
    Operation::new(crate::runtime::channels(), crate::runtime::decode_channels)
}

pub fn runtime_tunnels() -> Operation<Vec<TunnelRuntime>> {
    Operation::new(crate::runtime::tunnels(), crate::runtime::decode_tunnels)
}

pub fn runtime_agent_hosts() -> Operation<Vec<AgentRuntime>> {
    Operation::new(
        crate::runtime::agents_runtime(),
        crate::runtime::decode_agents_runtime,
    )
}

pub fn runtime_sync_channels() -> Operation<()> {
    Operation::new(crate::runtime::sync_channels(), decode_success)
}

pub fn runtime_reload_settings() -> Operation<()> {
    Operation::new(crate::runtime::reload_settings(), decode_success)
}

pub fn runtime_start_channel(kind: &str) -> Operation<()> {
    Operation::new(crate::runtime::start_channel(kind), decode_success)
}

pub fn runtime_stop_channel(kind: &str) -> Operation<()> {
    Operation::new(crate::runtime::stop_channel(kind), decode_success)
}

pub fn runtime_restart_channel(kind: &str) -> Operation<()> {
    Operation::new(crate::runtime::restart_channel(kind), decode_success)
}

pub fn runtime_kill_tunnel(provider: &str) -> Operation<()> {
    Operation::new(crate::runtime::kill_tunnel(provider), decode_success)
}

pub fn runtime_kill_agent(thread_id: &str) -> Operation<()> {
    Operation::new(crate::runtime::kill_agent(thread_id), decode_success)
}

pub fn runtime_kill_pty(session_id: &str) -> Operation<()> {
    Operation::new(crate::runtime::kill_pty(session_id), decode_success)
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::*;
    use crate::http::{AuthRequirement, HttpMethod};
    use crate::ResponseSpec;

    #[test]
    fn runtime_channels_pairs_request_and_decoder() {
        let op = runtime_channels();
        assert_eq!(op.request().method, HttpMethod::Get);
        assert_eq!(op.request().path, "/api/channels");
        assert_eq!(op.request().auth, AuthRequirement::BearerToken);

        let channels = op
            .decode(ResponseSpec::json(
                200,
                json!([{
                    "kind": "feishu",
                    "version": "0.1.0",
                    "plugin_dir": null,
                    "status": "running",
                    "reason": null
                }]),
            ))
            .expect("channels");
        assert_eq!(channels[0].kind, "feishu");
        assert_eq!(channels[0].instance_id, "feishu");
    }

    #[test]
    fn write_operations_pair_with_success_decoder() {
        let op = runtime_restart_channel("feishu");
        assert_eq!(op.request().method, HttpMethod::Post);
        assert_eq!(op.request().path, "/api/channels/feishu/restart");
        op.decode(ResponseSpec::json(204, Value::Null))
            .expect("success");
    }
}
