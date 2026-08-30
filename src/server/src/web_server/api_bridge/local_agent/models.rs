use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
use axum::extract::Path as AxumPath;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use uuid::Uuid;

use super::{json_error, launch_args_and_env, request_workspace, LOCAL_AGENT_CHANNEL_KIND};
use common::agent::AgentClientHandler;

pub async fn local_agent_models_handler(
    AxumPath((agent_id, profile_id)): AxumPath<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let agent_id = match super::local_agent_gate(&agent_id) {
        Ok(agent_id) => agent_id,
        Err(response) => return response,
    };
    if let Some(response) = super::direct_profile_response(&profile_id) {
        return response;
    }
    let workspace = request_workspace(&headers, &agent_id);
    match fetch_local_agent_models(&agent_id, &profile_id, &workspace).await {
        // An agent without a model selector legitimately serves zero models;
        // relay the empty list instead of inventing a failure.
        Ok(models) => local_agent_models_response(agent_id, profile_id, models),
        Err(message) => json_error(
            StatusCode::BAD_GATEWAY,
            &format!("failed to fetch model list: {message}"),
        ),
    }
}

fn local_agent_models_response(
    agent_id: String,
    profile_id: String,
    models: Vec<LocalAgentModel>,
) -> Response {
    let data: Vec<_> = models
        .into_iter()
        .map(|model| {
            let id = model.id;
            json!({
                "id": id.clone(),
                "model": id.clone(),
                "object": "model",
                "type": "model",
                "display_name": id.clone(),
                "displayName": id,
                "owned_by": "vibearound-local-agent",
                "created": 0,
                "created_at": null,
                "agent": agent_id,
                "profile": profile_id,
                "capabilities": {
                    "sessionless": true,
                    "streaming": true,
                }
            })
        })
        .collect();
    Json(json!({
        "object": "list",
        "data": data,
        "has_more": false,
        "source": "acp",
        "warning": null,
    }))
    .into_response()
}

async fn fetch_local_agent_models(
    agent_id: &str,
    profile_id: &str,
    workspace: &Path,
) -> Result<Vec<LocalAgentModel>, String> {
    // The gate already canonicalized the agent id; trust it.
    let agent_id = agent_id.to_string();
    let route = common::routing::RouteKey::new(
        LOCAL_AGENT_CHANNEL_KIND,
        format!("api_models_{}", Uuid::new_v4().simple()),
    );
    let (extra_args, env_vars) = launch_args_and_env(&agent_id, profile_id, workspace, &route)?;
    let handler = Arc::new(ModelListAgentClientHandler);
    let ready = common::agent::Agent::spawn(
        agent_id,
        &route,
        workspace,
        common::agent::StartupSession::Fresh,
        handler,
        extra_args,
        env_vars,
    )
    .await
    .map_err(|error| format!("{error:#}"))?;
    let agent = ready.agent;
    let result = async {
        let session = agent
            .new_session(acp::NewSessionRequest::new(workspace.to_path_buf()))
            .await?;
        Ok::<_, acp::Error>(models_from_acp_config_options(
            session.config_options.as_deref().unwrap_or_default(),
        ))
    }
    .await;
    agent.shutdown().await;
    result.map_err(|error| error.message.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LocalAgentModel {
    pub(super) id: String,
}

impl LocalAgentModel {
    fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

pub(super) fn models_from_acp_config_options(
    options: &[acp::SessionConfigOption],
) -> Vec<LocalAgentModel> {
    let mut seen = BTreeSet::new();
    let mut models = Vec::new();
    for option in options {
        if !is_model_config_option(option) {
            continue;
        }
        let acp::SessionConfigKind::Select(select) = &option.kind else {
            continue;
        };
        collect_acp_select_models(&select.options, &mut seen, &mut models);
        push_acp_model(&mut seen, &mut models, select.current_value.to_string());
    }
    models
}

fn is_model_config_option(option: &acp::SessionConfigOption) -> bool {
    matches!(
        option.category,
        Some(acp::SessionConfigOptionCategory::Model)
    ) || option.id.to_string().eq_ignore_ascii_case("model")
}

fn collect_acp_select_models(
    options: &acp::SessionConfigSelectOptions,
    seen: &mut BTreeSet<String>,
    models: &mut Vec<LocalAgentModel>,
) {
    match options {
        acp::SessionConfigSelectOptions::Ungrouped(options) => {
            for option in options {
                push_acp_model(seen, models, option.value.to_string());
            }
        }
        acp::SessionConfigSelectOptions::Grouped(groups) => {
            for group in groups {
                for option in &group.options {
                    push_acp_model(seen, models, option.value.to_string());
                }
            }
        }
        _ => {}
    }
}

fn push_acp_model(seen: &mut BTreeSet<String>, models: &mut Vec<LocalAgentModel>, id: String) {
    let id = id.trim().to_string();
    if id.is_empty() || !seen.insert(id.clone()) {
        return;
    }
    models.push(LocalAgentModel::new(id));
}

struct ModelListAgentClientHandler;

#[async_trait::async_trait]
impl AgentClientHandler for ModelListAgentClientHandler {
    async fn session_notification(&self, _args: acp::SessionNotification) -> acp::Result<()> {
        Ok(())
    }

    async fn request_permission(
        &self,
        _args: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        Ok(acp::RequestPermissionResponse::new(
            acp::RequestPermissionOutcome::Cancelled,
        ))
    }
}
