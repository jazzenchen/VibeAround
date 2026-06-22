//! Ready-to-send operation catalog.
//!
//! These helpers pair each request builder with its matching decoder. They
//! still do not send anything; the host owns transport.

use serde_json::Value;

use crate::launcher::{LaunchPlanBody, LaunchPlanResponse, LauncherPreferencesResponse};
use crate::operation::Operation;
use crate::previews::PreviewsResponse;
use crate::profiles::{ModelProfileDraft, ModelProfileSummary, ProfileDef, ProfileLaunchOption};
use crate::runtime::{AgentRuntime, AgentsConfig, ChannelRuntime, TunnelRuntime};
use crate::service::{ServiceHealthResponse, ServiceInfoResponse};
use crate::sessions::{
    CreateSessionBody, CreateSessionResponse, LaunchSessionInfo, LaunchSessionsQuery,
    SessionListItem, TmuxSessionsResponse,
};
use crate::settings::SettingsWriteResponse;
use crate::workspaces::{CreateWorkspaceResponse, WorkspacesResponse};
use crate::{ResponseSpec, Result};

pub fn service_health() -> Operation<ServiceHealthResponse> {
    Operation::new(crate::service::health(), crate::service::decode_health)
}

pub fn service_info() -> Operation<ServiceInfoResponse> {
    Operation::new(crate::service::info(), crate::service::decode_info)
}

pub fn settings_get() -> Operation<Value> {
    Operation::new(crate::settings::get(), crate::settings::decode_get)
}

pub fn settings_put(settings: Value) -> Result<Operation<SettingsWriteResponse>> {
    Ok(Operation::new(
        crate::settings::put(settings)?,
        crate::settings::decode_put,
    ))
}

pub fn workspaces() -> Operation<WorkspacesResponse> {
    Operation::new(crate::workspaces::list(), crate::workspaces::decode_list)
}

pub fn workspace_create(name: &str) -> Result<Operation<CreateWorkspaceResponse>> {
    Ok(Operation::new(
        crate::workspaces::create(name)?,
        crate::workspaces::decode_create,
    ))
}

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

pub fn launcher_preferences() -> Operation<LauncherPreferencesResponse> {
    Operation::new(
        crate::launcher::preferences(),
        crate::launcher::decode_preferences,
    )
}

pub fn launcher_plan(body: LaunchPlanBody<'_>) -> Result<Operation<LaunchPlanResponse>> {
    Ok(Operation::new(
        crate::launcher::plan(body)?,
        crate::launcher::decode_plan,
    ))
}

pub fn sessions() -> Operation<Vec<SessionListItem>> {
    Operation::new(crate::sessions::list(), crate::sessions::decode_list)
}

pub fn session_create(body: CreateSessionBody<'_>) -> Result<Operation<CreateSessionResponse>> {
    Ok(Operation::new(
        crate::sessions::create(body)?,
        crate::sessions::decode_create,
    ))
}

pub fn tmux_sessions() -> Operation<TmuxSessionsResponse> {
    Operation::new(
        crate::sessions::list_tmux(),
        crate::sessions::decode_list_tmux,
    )
}

pub fn launch_sessions(
    agent_id: &str,
    query: LaunchSessionsQuery<'_>,
) -> Operation<Vec<LaunchSessionInfo>> {
    Operation::new(
        crate::sessions::list_launch_sessions(agent_id, query),
        crate::sessions::decode_launch_sessions,
    )
}

pub fn launch_sessions_batch(
    agent_ids: &[&str],
    workspace_paths: &[&str],
    include_archived: Option<bool>,
    limit: Option<usize>,
) -> Result<Operation<Vec<LaunchSessionInfo>>> {
    Ok(Operation::new(
        crate::sessions::list_launch_sessions_batch(
            agent_ids,
            workspace_paths,
            include_archived,
            limit,
        )?,
        crate::sessions::decode_launch_sessions,
    ))
}

pub fn profile_launch_options() -> Operation<Vec<ProfileLaunchOption>> {
    Operation::new(
        crate::profiles::list_launch_options(),
        crate::profiles::decode_launch_options,
    )
}

pub fn model_profiles() -> Operation<Vec<ModelProfileSummary>> {
    Operation::new(
        crate::profiles::list_model_profiles(),
        crate::profiles::decode_model_profiles,
    )
}

pub fn model_profile(id: &str) -> Operation<ProfileDef> {
    Operation::new(
        crate::profiles::get_model_profile(id),
        crate::profiles::decode_model_profile,
    )
}

pub fn model_profile_create(draft: &ModelProfileDraft) -> Result<Operation<ProfileDef>> {
    Ok(Operation::new(
        crate::profiles::create_model_profile(draft)?,
        crate::profiles::decode_model_profile,
    ))
}

pub fn model_profile_update(id: &str, profile: &ProfileDef) -> Result<Operation<ProfileDef>> {
    Ok(Operation::new(
        crate::profiles::update_model_profile(id, profile)?,
        crate::profiles::decode_model_profile,
    ))
}

pub fn previews() -> Operation<PreviewsResponse> {
    Operation::new(crate::previews::list(), crate::previews::decode_list)
}

pub fn decode_success(response: ResponseSpec) -> Result<()> {
    response.ensure_success()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::http::{AuthRequirement, HttpMethod};

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
    }

    #[test]
    fn dynamic_model_profile_operation_encodes_path() {
        let op = model_profile("openai/default");
        assert_eq!(op.request().path, "/api/model-profiles/openai%2Fdefault");
    }
}
