//! Bridge handler for spawned subagents.
//!
//! Subagent output is web-only for now. The host thread remains the only
//! participant in IM/stdio channels; subagents stream into the web multi-agent
//! panel and permission requests are forwarded through the existing web queue.

use std::sync::{Arc, Weak};

use agent_client_protocol::schema as acp;

use crate::agent::AgentClientHandler;
use crate::routing::RouteKey;
use crate::workspace::threads::{ThreadAgent, WorkspaceThreadId};
use crate::workspace::WorkspaceThreadManager;

use super::plugin_host::PluginHost;
use super::types::ChannelOutput;

pub struct SubagentBridgeHandler {
    plugin_host: Arc<PluginHost>,
    workspace_threads: Weak<WorkspaceThreadManager>,
    thread_id: WorkspaceThreadId,
    thread_agent: ThreadAgent,
}

impl SubagentBridgeHandler {
    pub fn for_thread(
        plugin_host: Arc<PluginHost>,
        workspace_threads: &Arc<WorkspaceThreadManager>,
        thread_id: WorkspaceThreadId,
        thread_agent: ThreadAgent,
    ) -> Self {
        Self {
            plugin_host,
            workspace_threads: Arc::downgrade(workspace_threads),
            thread_id,
            thread_agent,
        }
    }

    async fn attached_web_routes(&self) -> Vec<RouteKey> {
        let Some(workspace_threads) = self.workspace_threads.upgrade() else {
            return Vec::new();
        };
        match workspace_threads
            .attached_routes_for_thread(&self.thread_id)
            .await
        {
            Ok(routes) => routes
                .into_iter()
                .filter(|route| route.channel_kind == "web")
                .collect(),
            Err(error) => {
                tracing::warn!(
                    thread_id = %self.thread_id,
                    error = %error,
                    "failed to resolve web routes for subagent output"
                );
                Vec::new()
            }
        }
    }
}

#[async_trait::async_trait]
impl AgentClientHandler for SubagentBridgeHandler {
    async fn session_notification(&self, args: acp::SessionNotification) -> acp::Result<()> {
        let payload = serde_json::to_value(&args)
            .map_err(|e| acp::Error::new(-32603, format!("serialize: {}", e)))?;

        for route in self.attached_web_routes().await {
            self.plugin_host
                .send_output(ChannelOutput::SubagentAcp {
                    route,
                    agent: self.thread_agent.clone(),
                    payload: payload.clone(),
                })
                .await;
        }
        Ok(())
    }

    async fn request_permission(
        &self,
        args: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        if args.options.is_empty() {
            return Err(acp::Error::method_not_found());
        }

        let routes = self.attached_web_routes().await;
        let Some(first_route) = routes.first() else {
            return Ok(acp::RequestPermissionResponse::new(
                acp::RequestPermissionOutcome::Cancelled,
            ));
        };

        let request_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = tokio::sync::oneshot::channel::<acp::RequestPermissionResponse>();
        self.plugin_host
            .pending_permissions
            .insert(request_id.clone(), (first_route.channel_kind.clone(), tx));

        let mut payload = serde_json::to_value(&args).map_err(|e| {
            self.plugin_host.pending_permissions.remove(&request_id);
            acp::Error::new(-32603, format!("serialize requestPermission: {}", e))
        })?;
        if let Some(object) = payload.as_object_mut() {
            object.insert(
                "subagent".to_string(),
                serde_json::json!({
                    "id": self.thread_agent.id.to_string(),
                    "name": self.thread_agent.name.clone(),
                    "turn_id": self.thread_agent.turn_id.to_string(),
                }),
            );
        }

        for route in routes {
            self.plugin_host
                .send_output(ChannelOutput::PermissionRequest {
                    route,
                    request_id: request_id.clone(),
                    payload: payload.clone(),
                })
                .await;
        }

        match rx.await {
            Ok(response) => Ok(response),
            Err(_) => {
                self.plugin_host.pending_permissions.remove(&request_id);
                Ok(acp::RequestPermissionResponse::new(
                    acp::RequestPermissionOutcome::Cancelled,
                ))
            }
        }
    }
}
