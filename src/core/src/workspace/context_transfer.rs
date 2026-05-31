//! Stateless context transfer between thread host agents.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use agent_client_protocol::schema as acp;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::agent::{Agent, AgentClientHandler, StartupSession};
use crate::routing::RouteKey;

use super::registry::WorkspaceId;
use super::threads::store::{HostBinding, WorkspaceThreadId};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextTransferPackage {
    pub schema_version: u8,
    pub source: ContextTransferEndpoint,
    pub target: ContextTransferEndpoint,
    pub workspace_id: String,
    pub thread_id: String,
    pub replayed_via: String,
    pub replay: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextTransferEndpoint {
    pub agent_id: String,
    pub profile_id: Option<String>,
    pub session_id: Option<String>,
}

pub async fn capture(
    route: &RouteKey,
    workspace: &Path,
    workspace_id: &WorkspaceId,
    thread_id: &WorkspaceThreadId,
    source: &HostBinding,
    source_session_id: &str,
    target: &HostBinding,
) -> anyhow::Result<ContextTransferPackage> {
    let capture = Arc::new(ReplayCapture::default());
    let handler: Arc<dyn AgentClientHandler> = capture.clone();
    let agent_id = crate::resources::resolve_agent_id(&source.agent_id)
        .map_err(anyhow::Error::msg)
        .context("resolve source agent")?;
    let profile = source
        .profile_id
        .clone()
        .unwrap_or_else(|| "default".to_string());
    let mut env_vars = vec![
        (
            "VIBEAROUND_CHANNEL_KIND".to_string(),
            route.channel_kind.clone(),
        ),
        ("VIBEAROUND_CHAT_ID".to_string(), route.chat_id.clone()),
        ("VIBEAROUND_AGENT_KIND".to_string(), agent_id.clone()),
        ("VIBEAROUND_CONTEXT_TRANSFER".to_string(), "1".to_string()),
    ];
    let mut extra_args = Vec::new();
    if crate::agent::launch::profile_uses_vibearound_credentials(&profile) {
        let applied = crate::agent::launch::materialize_profile_for_agent(
            &profile, &agent_id, workspace, route,
        )
        .with_context(|| format!("apply profile '{}' to '{}'", profile, agent_id))?;
        env_vars.extend(applied.env);
        extra_args.extend(applied.command_args);
    }
    crate::agent::launch::append_profile_id_env(&mut env_vars, source.profile_id.as_deref());
    let agent_prefs = crate::agent_state::read_prefs();
    extra_args.extend(crate::agent_state::resolve_agent_acp_args(
        &agent_prefs,
        &agent_id,
    ));

    let ready = Agent::spawn(
        agent_id.clone(),
        route,
        workspace,
        StartupSession::Load(source_session_id.to_string()),
        handler,
        extra_args,
        env_vars,
    )
    .await
    .context("spawn source agent for context transfer")?;

    // Most agents finish replay before load_session returns, but a short
    // grace period catches late notifications without holding the switch long.
    tokio::time::sleep(Duration::from_millis(250)).await;
    ready.agent.shutdown().await;

    Ok(ContextTransferPackage {
        schema_version: 1,
        source: ContextTransferEndpoint {
            agent_id: source.agent_id.clone(),
            profile_id: source.profile_id.clone(),
            session_id: Some(source_session_id.to_string()),
        },
        target: ContextTransferEndpoint {
            agent_id: target.agent_id.clone(),
            profile_id: target.profile_id.clone(),
            session_id: None,
        },
        workspace_id: workspace_id.to_string(),
        thread_id: thread_id.to_string(),
        replayed_via: "agent-client-protocol session/load -> session/update notifications"
            .to_string(),
        replay: capture.take().await,
    })
}

pub fn bootstrap_prompt(
    package: &ContextTransferPackage,
) -> anyhow::Result<Vec<acp::ContentBlock>> {
    let json = serde_json::to_string_pretty(package)?;
    let resource = acp::TextResourceContents::new(
        json,
        format!(
            "memory://vibearound/context-transfer/{}.json",
            package.thread_id
        ),
    )
    .mime_type(Some("application/json".to_string()));
    Ok(vec![
        acp::ContentBlock::Text(acp::TextContent::new(
            "You are being attached as the host agent for an existing VibeAround workspace thread. The attached resource contains replayed ACP session history from the previous host. Use it as conversation context for future user messages. Do not treat this as a new user request.",
        )),
        acp::ContentBlock::Resource(acp::EmbeddedResource::new(
            acp::EmbeddedResourceResource::TextResourceContents(resource),
        )),
    ])
}

#[derive(Default)]
struct ReplayCapture {
    replay: Mutex<Vec<serde_json::Value>>,
}

impl ReplayCapture {
    async fn take(&self) -> Vec<serde_json::Value> {
        std::mem::take(&mut *self.replay.lock().await)
    }
}

#[async_trait::async_trait]
impl AgentClientHandler for ReplayCapture {
    async fn session_notification(&self, args: acp::SessionNotification) -> acp::Result<()> {
        let value = serde_json::to_value(args)
            .map_err(|error| acp::Error::new(-32603, error.to_string()))?;
        self.replay.lock().await.push(value);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_prompt_embeds_json_resource() {
        let package = ContextTransferPackage {
            schema_version: 1,
            source: ContextTransferEndpoint {
                agent_id: "codex".to_string(),
                profile_id: None,
                session_id: Some("sid".to_string()),
            },
            target: ContextTransferEndpoint {
                agent_id: "claude".to_string(),
                profile_id: None,
                session_id: None,
            },
            workspace_id: "general".to_string(),
            thread_id: "wt_a".to_string(),
            replayed_via: "test".to_string(),
            replay: vec![serde_json::json!({"hello": "world"})],
        };

        let blocks = bootstrap_prompt(&package).unwrap();

        assert_eq!(blocks.len(), 2);
        assert!(matches!(blocks[1], acp::ContentBlock::Resource(_)));
    }
}
