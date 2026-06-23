use serde::Serialize;
use serde_json::Value;

use crate::error::encode_body;
use crate::http::{AuthRequirement, HttpMethod, RequestSpec, ResponseSpec};
use crate::Result;

use super::{
    AgentLaunchArgsBody, AgentProfileBody, EnabledBody, LaunchPlanBody, LaunchPlanResponse,
    LauncherPreferencesResponse, ProfileConnectionBody, SelectedAgentBody,
};

pub fn preferences() -> RequestSpec {
    RequestSpec::new(
        HttpMethod::Get,
        "/api/launcher/preferences",
        AuthRequirement::BearerToken,
    )
}

pub fn decode_preferences(response: ResponseSpec) -> Result<LauncherPreferencesResponse> {
    response.decode()
}

pub fn set_default_agent(agent_id: &str, profile_id: Option<&str>) -> Result<RequestSpec> {
    launcher_put(
        "/api/launcher/default-agent",
        AgentProfileBody {
            agent_id,
            profile_id,
        },
    )
}

pub fn set_agent_profile(agent_id: &str, profile_id: Option<&str>) -> Result<RequestSpec> {
    launcher_put(
        "/api/launcher/agent-profile",
        AgentProfileBody {
            agent_id,
            profile_id,
        },
    )
}

pub fn set_agent_launch_args(agent_id: &str, launch_args: Value) -> Result<RequestSpec> {
    launcher_put(
        "/api/launcher/agent-launch-args",
        AgentLaunchArgsBody {
            agent_id,
            launch_args,
        },
    )
}

pub fn set_selected_agent(agent_id: &str) -> Result<RequestSpec> {
    launcher_put(
        "/api/launcher/selected-agent",
        SelectedAgentBody { agent_id },
    )
}

pub fn set_local_agent_api(enabled: bool) -> Result<RequestSpec> {
    launcher_put("/api/launcher/local-agent-api", EnabledBody { enabled })
}

pub fn set_profile_connection(
    profile_id: &str,
    agent_id: &str,
    preference: Value,
) -> Result<RequestSpec> {
    launcher_put(
        "/api/launcher/profile-connection",
        ProfileConnectionBody {
            profile_id,
            agent_id,
            preference,
        },
    )
}

pub fn plan(body: LaunchPlanBody<'_>) -> Result<RequestSpec> {
    Ok(RequestSpec::new(
        HttpMethod::Post,
        "/api/launcher/plan",
        AuthRequirement::BearerToken,
    )
    .with_body(encode_body(body)?))
}

pub fn decode_plan(response: ResponseSpec) -> Result<LaunchPlanResponse> {
    response.decode()
}

fn launcher_put<T: Serialize>(path: &'static str, body: T) -> Result<RequestSpec> {
    Ok(
        RequestSpec::new(HttpMethod::Put, path, AuthRequirement::BearerToken)
            .with_body(encode_body(body)?),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn selected_agent_request_uses_camel_case_body() {
        let request = set_selected_agent("codex").expect("request");
        assert_eq!(request.method, HttpMethod::Put);
        assert_eq!(request.path, "/api/launcher/selected-agent");
        assert_eq!(request.body, Some(json!({ "agentId": "codex" })));
    }

    #[test]
    fn launch_plan_request_preserves_null_choices() {
        let request = plan(LaunchPlanBody {
            agent_id: Some("claude"),
            profile_id: None,
            launch_target: Some("anthropic"),
            session_id: None,
        })
        .expect("request");
        assert_eq!(request.method, HttpMethod::Post);
        assert_eq!(request.path, "/api/launcher/plan");
        assert_eq!(
            request.body,
            Some(json!({
                "agentId": "claude",
                "profileId": null,
                "launchTarget": "anthropic",
                "sessionId": null
            }))
        );
    }
}
