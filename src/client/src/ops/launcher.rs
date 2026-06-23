use serde_json::Value;

use crate::launcher::{LaunchPlanBody, LaunchPlanResponse, LauncherPreferencesResponse};
use crate::operation::Operation;
use crate::Result;

pub fn launcher_preferences() -> Operation<LauncherPreferencesResponse> {
    Operation::new(
        crate::launcher::preferences(),
        crate::launcher::decode_preferences,
    )
}

pub fn launcher_set_default_agent(
    agent_id: &str,
    profile_id: Option<&str>,
) -> Result<Operation<LauncherPreferencesResponse>> {
    Ok(Operation::new(
        crate::launcher::set_default_agent(agent_id, profile_id)?,
        crate::launcher::decode_preferences,
    ))
}

pub fn launcher_set_agent_profile(
    agent_id: &str,
    profile_id: Option<&str>,
) -> Result<Operation<LauncherPreferencesResponse>> {
    Ok(Operation::new(
        crate::launcher::set_agent_profile(agent_id, profile_id)?,
        crate::launcher::decode_preferences,
    ))
}

pub fn launcher_set_agent_launch_args(
    agent_id: &str,
    launch_args: Value,
) -> Result<Operation<LauncherPreferencesResponse>> {
    Ok(Operation::new(
        crate::launcher::set_agent_launch_args(agent_id, launch_args)?,
        crate::launcher::decode_preferences,
    ))
}

pub fn launcher_set_selected_agent(
    agent_id: &str,
) -> Result<Operation<LauncherPreferencesResponse>> {
    Ok(Operation::new(
        crate::launcher::set_selected_agent(agent_id)?,
        crate::launcher::decode_preferences,
    ))
}

pub fn launcher_set_local_agent_api(
    enabled: bool,
) -> Result<Operation<LauncherPreferencesResponse>> {
    Ok(Operation::new(
        crate::launcher::set_local_agent_api(enabled)?,
        crate::launcher::decode_preferences,
    ))
}

pub fn launcher_set_profile_connection(
    profile_id: &str,
    agent_id: &str,
    preference: Value,
) -> Result<Operation<LauncherPreferencesResponse>> {
    Ok(Operation::new(
        crate::launcher::set_profile_connection(profile_id, agent_id, preference)?,
        crate::launcher::decode_preferences,
    ))
}

pub fn launcher_plan(body: LaunchPlanBody<'_>) -> Result<Operation<LaunchPlanResponse>> {
    Ok(Operation::new(
        crate::launcher::plan(body)?,
        crate::launcher::decode_plan,
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::http::HttpMethod;
    use crate::ResponseSpec;

    #[test]
    fn launcher_write_operations_decode_preferences() {
        let op = launcher_set_selected_agent("codex").expect("operation");
        assert_eq!(op.request().method, HttpMethod::Put);
        assert_eq!(op.request().path, "/api/launcher/selected-agent");
        assert_eq!(op.request().body, Some(json!({ "agentId": "codex" })));

        let preferences = op
            .decode(ResponseSpec::json(
                200,
                json!({
                    "selectedAgent": "codex",
                    "defaultAgent": "codex",
                    "defaultProfileId": null,
                    "enabledAgents": ["codex"],
                    "agentPreferences": {},
                    "localAgentApiEnabled": true,
                    "profileConnections": {}
                }),
            ))
            .expect("decode");
        assert_eq!(preferences.selected_agent, "codex");
    }
}
