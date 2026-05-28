//! `ChannelBridgeHandler` — the downstream handler wired to the upstream
//! `Agent`. Its two jobs:
//!
//! 1. **`session_notification`** — wrap every `acp::SessionNotification`
//!    from the agent as a workspace-thread reply, then fan it out to attached
//!    channel routes.
//! 2. **`request_permission`** — turn an ACP `requestPermission` call from
//!    the upstream agent into a `ChannelOutput::PermissionRequest` to the
//!    plugin, then await the plugin's reply via a per-request oneshot
//!    registered in `PluginHost::pending_permissions`. No timeout — the UX
//!    is "user takes as long as they need".

use std::sync::{Arc, Weak};

use agent_client_protocol::schema as acp;
use tokio::sync::{mpsc, Mutex};

use crate::agent::AgentClientHandler;
use crate::routing::RouteKey;
use crate::workspace::registry::WorkspaceId;
use crate::workspace::threads::store::{HostBinding, WorkspaceThreadId};
use crate::workspace::threads::{ThreadAgent, ThreadAgentId};
use crate::workspace::WorkspaceThreadManager;

use super::agent_protocol::{
    notification_payload, notification_payload_with_text, session_update_text,
    synthetic_agent_message_payload, synthetic_user_message_payload, AgentProtocolFilter,
};
use super::plugin_host::PluginHost;
use super::types::{ChannelOutput, ThreadReply, ThreadReplyAgent, ThreadReplyPayload};

pub(crate) struct ChannelBridgeHandler {
    plugin_host: Arc<PluginHost>,
    workspace_threads: Weak<WorkspaceThreadManager>,
    workspace_id: WorkspaceId,
    thread_id: WorkspaceThreadId,
    host_binding: HostBinding,
    host_protocol: Mutex<HostProtocolState>,
}

impl ChannelBridgeHandler {
    pub(crate) fn for_thread(
        plugin_host: Arc<PluginHost>,
        workspace_threads: &Arc<WorkspaceThreadManager>,
        workspace_id: WorkspaceId,
        thread_id: WorkspaceThreadId,
        host_binding: HostBinding,
    ) -> Self {
        Self {
            plugin_host,
            workspace_threads: Arc::downgrade(workspace_threads),
            workspace_id,
            thread_id,
            host_binding,
            host_protocol: Mutex::new(HostProtocolState::default()),
        }
    }

    async fn attached_routes(&self) -> Vec<RouteKey> {
        let Some(workspace_threads) = self.workspace_threads.upgrade() else {
            return Vec::new();
        };
        match workspace_threads
            .attached_routes_for_thread(&self.thread_id)
            .await
        {
            Ok(routes) => routes,
            Err(error) => {
                tracing::warn!(
                    "[ChannelBridgeHandler] failed to resolve attached routes thread={}: {:#}",
                    self.thread_id,
                    error
                );
                Vec::new()
            }
        }
    }

    async fn attached_web_routes(&self) -> Vec<RouteKey> {
        self.attached_routes()
            .await
            .into_iter()
            .filter(|route| route.channel_kind == "web")
            .collect()
    }

    async fn filter_host_protocol_notification(
        &self,
        args: &acp::SessionNotification,
    ) -> acp::Result<Option<serde_json::Value>> {
        let Some(text) = session_update_text(&args.update) else {
            return notification_payload(args).map(Some);
        };
        let visible = {
            let mut state = self.host_protocol.lock().await;
            state.last_session_id = Some(args.session_id.to_string());
            state.filter.feed_text(text)
        };
        if visible.is_empty() {
            return Ok(None);
        }
        notification_payload_with_text(args, visible).map(Some)
    }

    async fn finish_host_protocol(&self, success: bool) {
        let (session_id, finished) = {
            let mut state = self.host_protocol.lock().await;
            let session_id = state.last_session_id.clone();
            let finished = state.filter.finish();
            (session_id, finished)
        };

        if let Some(session_id) = session_id.as_deref() {
            self.send_host_visible_text_chunk(session_id, &finished.visible_tail)
                .await;
        }
        if !success {
            return;
        }
        let Some(frame) = finished.frame else {
            return;
        };
        let envelope = match frame {
            Ok(envelope) => envelope,
            Err(error) => {
                self.send_system_text(&format!("Subagent assignment ignored: {}", error))
                    .await;
                return;
            }
        };
        self.dispatch_host_protocol_envelope(&envelope).await;
    }

    async fn dispatch_host_protocol_envelope(&self, envelope: &str) {
        let assignment = match HostAssignment::parse(envelope) {
            Ok(Some(assignment)) => assignment,
            Ok(None) => return,
            Err(error) => {
                self.send_system_text(&format!("Subagent assignment ignored: {}", error))
                    .await;
                return;
            }
        };
        let Some(workspace_threads) = self.workspace_threads.upgrade() else {
            self.send_system_text("Subagent assignment ignored: thread manager is unavailable.")
                .await;
            return;
        };
        let runtime = match workspace_threads
            .runtime_for_thread_id(&self.thread_id)
            .await
        {
            Ok(runtime) => runtime,
            Err(error) => {
                self.send_system_text(&format!(
                    "Subagent assignment ignored: failed to load thread runtime: {:#}",
                    error
                ))
                .await;
                return;
            }
        };
        let status_tx = self.spawn_subagent_status_forwarder();
        let to_agent_id = assignment.to_agent_id.clone();
        let task = assignment.task.clone();
        if let Err(error) = runtime
            .prompt_subagent_assignment(&to_agent_id, assignment.payload, status_tx)
            .await
        {
            self.send_system_text(&format!(
                "Subagent assignment ignored for {}: {}",
                to_agent_id, error.message
            ))
            .await;
        } else {
            self.send_subagent_assignment_visible(&to_agent_id, &task)
                .await;
        }
    }

    async fn send_host_visible_text_chunk(&self, session_id: &str, text: &str) {
        if text.is_empty() {
            return;
        }
        let payload = synthetic_agent_message_payload(session_id, text.to_string());
        let reply = ThreadReply {
            workspace_id: self.workspace_id.to_string(),
            thread_id: self.thread_id.to_string(),
            agent: ThreadReplyAgent {
                id: self.host_binding.agent_id.clone(),
                profile: self.host_binding.profile_id.clone(),
                session_id: session_id.to_string(),
            },
            payload: ThreadReplyPayload::AcpSessionNotification {
                notification: payload,
            },
        };

        for route in self.attached_routes().await {
            self.plugin_host
                .send_output(ChannelOutput::ThreadReply {
                    route,
                    reply: reply.clone(),
                })
                .await;
        }
    }

    async fn send_subagent_assignment_visible(&self, agent_id: &ThreadAgentId, task: &str) {
        let Some(workspace_threads) = self.workspace_threads.upgrade() else {
            return;
        };
        let Ok(runtime) = workspace_threads
            .runtime_for_thread_id(&self.thread_id)
            .await
        else {
            return;
        };
        let Some(agent) = runtime
            .state()
            .await
            .agents
            .into_iter()
            .find(|agent| &agent.id == agent_id)
        else {
            return;
        };
        let text = if task.trim().is_empty() {
            "Host assignment received.".to_string()
        } else {
            format!("Host assignment:\n\n{}", task.trim())
        };
        let payload = synthetic_user_message_payload(&format!("subagent:{}", agent.id), text);
        for route in self.attached_web_routes().await {
            self.plugin_host
                .send_output(ChannelOutput::SubagentAcp {
                    route,
                    agent: agent.clone(),
                    payload: payload.clone(),
                })
                .await;
        }
    }

    fn spawn_subagent_status_forwarder(&self) -> mpsc::UnboundedSender<ThreadAgent> {
        let (tx, mut rx) = mpsc::unbounded_channel::<ThreadAgent>();
        let plugin_host = Arc::clone(&self.plugin_host);
        let workspace_threads = self.workspace_threads.clone();
        let thread_id = self.thread_id.clone();
        tokio::spawn(async move {
            while let Some(agent) = rx.recv().await {
                let Some(workspace_threads) = workspace_threads.upgrade() else {
                    continue;
                };
                let routes = match workspace_threads
                    .attached_routes_for_thread(&thread_id)
                    .await
                {
                    Ok(routes) => routes,
                    Err(error) => {
                        tracing::warn!(
                            thread_id = %thread_id,
                            error = %error,
                            "failed to resolve web routes for subagent status"
                        );
                        continue;
                    }
                };
                for route in routes
                    .into_iter()
                    .filter(|route| route.channel_kind == "web")
                {
                    plugin_host
                        .send_output(ChannelOutput::SubagentStatus {
                            route,
                            agent: agent.clone(),
                        })
                        .await;
                }
            }
        });
        tx
    }

    async fn send_system_text(&self, text: &str) {
        for route in self.attached_web_routes().await {
            self.plugin_host
                .send_output(ChannelOutput::SystemText {
                    route,
                    text: text.to_string(),
                    reply_to: None,
                })
                .await;
        }
    }
}

#[async_trait::async_trait]
impl AgentClientHandler for ChannelBridgeHandler {
    async fn session_notification(&self, args: acp::SessionNotification) -> acp::Result<()> {
        let Some(payload) = self.filter_host_protocol_notification(&args).await? else {
            return Ok(());
        };

        // Log the update variant so we can tell whether an agent is emitting
        // real assistant text or only tool/thinking chunks. Claude Agent
        // v0.25.x sometimes end-turns after only tool_call updates and never
        // yields a user-visible message; this log surfaces that case.
        let update_kind = payload
            .get("update")
            .and_then(|u| u.get("sessionUpdate"))
            .and_then(|v| v.as_str())
            .unwrap_or("<none>");
        let preview = payload
            .get("update")
            .and_then(|u| u.get("content"))
            .and_then(|c| c.get("text"))
            .and_then(|v| v.as_str())
            .map(|s| s.chars().take(60).collect::<String>())
            .unwrap_or_default();
        tracing::info!(
            "[ChannelBridgeHandler] session_notification thread={} session={} kind={} preview={:?}",
            self.thread_id,
            args.session_id,
            update_kind,
            preview
        );

        let reply = ThreadReply {
            workspace_id: self.workspace_id.to_string(),
            thread_id: self.thread_id.to_string(),
            agent: ThreadReplyAgent {
                id: self.host_binding.agent_id.clone(),
                profile: self.host_binding.profile_id.clone(),
                session_id: args.session_id.to_string(),
            },
            payload: ThreadReplyPayload::AcpSessionNotification {
                notification: payload,
            },
        };

        for route in self.attached_routes().await {
            self.plugin_host
                .send_output(ChannelOutput::ThreadReply {
                    route,
                    reply: reply.clone(),
                })
                .await;
        }
        Ok(())
    }

    async fn prompt_finished(&self, success: bool) -> acp::Result<()> {
        self.finish_host_protocol(success).await;
        Ok(())
    }

    async fn request_permission(
        &self,
        args: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        if args.options.is_empty() {
            return Err(acp::Error::method_not_found());
        }

        // Register a oneshot keyed by a fresh request_id, tagged with this
        // channel kind. The plugin-bridge forwarder task consumes it once
        // the plugin's ACP response arrives. The tag lets
        // `PluginHost::cancel_channel_permissions` drain orphaned entries
        // when the plugin dies, so `rx.await` below resolves as `Cancelled`
        // instead of stalling the agent turn.
        let request_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = tokio::sync::oneshot::channel::<acp::RequestPermissionResponse>();
        let routes = self.attached_routes().await;
        let Some(first_route) = routes.first() else {
            return Ok(acp::RequestPermissionResponse::new(
                acp::RequestPermissionOutcome::Cancelled,
            ));
        };
        self.plugin_host
            .pending_permissions
            .insert(request_id.clone(), (first_route.channel_kind.clone(), tx));

        let options_len = args.options.len();
        let payload = match serde_json::to_value(&args) {
            Ok(v) => v,
            Err(e) => {
                self.plugin_host.pending_permissions.remove(&request_id);
                return Err(acp::Error::new(
                    -32603,
                    format!("serialize requestPermission: {}", e),
                ));
            }
        };

        tracing::info!(
            "[ChannelBridgeHandler] request_permission forwarding thread={} routes={} request_id={} options={}",
            self.thread_id,
            routes.len(),
            request_id,
            options_len
        );

        for route in routes {
            self.plugin_host
                .send_output(ChannelOutput::PermissionRequest {
                    route,
                    request_id: request_id.clone(),
                    payload: payload.clone(),
                })
                .await;
        }

        // Wait for plugin response — no timeout by design. If the plugin
        // crashes, `tx` is dropped and `rx.await` errors, which we treat as
        // cancelled so the upstream agent turn gracefully ends.
        match rx.await {
            Ok(response) => Ok(response),
            Err(_) => {
                self.plugin_host.pending_permissions.remove(&request_id);
                tracing::info!(
                    "[ChannelBridgeHandler] request_permission dropped (plugin gone?) thread={} request_id={}",
                    self.thread_id, request_id
                );
                Ok(acp::RequestPermissionResponse::new(
                    acp::RequestPermissionOutcome::Cancelled,
                ))
            }
        }
    }
}

#[derive(Default)]
struct HostProtocolState {
    filter: AgentProtocolFilter,
    last_session_id: Option<String>,
}

struct HostAssignment {
    to_agent_id: ThreadAgentId,
    payload: serde_json::Value,
    task: String,
}

impl HostAssignment {
    fn parse(envelope: &str) -> Result<Option<Self>, String> {
        let payload: serde_json::Value = serde_json::from_str(envelope)
            .map_err(|error| format!("invalid va-agent-protocol JSON: {}", error))?;
        let object = payload
            .as_object()
            .ok_or_else(|| "va-agent-protocol payload must be a JSON object".to_string())?;
        let protocol = string_field(object, "protocol")?;
        if protocol != "va-agent-protocol" {
            return Err(format!(
                "protocol field expected `va-agent-protocol`, got `{}`",
                protocol
            ));
        }
        let kind = string_field(object, "kind")?;
        if kind != "assignment" {
            return Ok(None);
        }
        let to_agent_id = string_field(object, "to_agent_id")?;
        if to_agent_id.trim().is_empty() {
            return Err("assignment field `to_agent_id` must not be empty".to_string());
        }
        Ok(Some(Self {
            to_agent_id: ThreadAgentId::from(to_agent_id),
            task: object
                .get("task")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string(),
            payload,
        }))
    }
}

fn string_field(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<String, String> {
    object
        .get(field)
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("va-agent-protocol payload missing string field `{}`", field))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_assignment_parses_target_agent_id() {
        let assignment = HostAssignment::parse(
            r#"{"protocol":"va-agent-protocol","kind":"assignment","to_agent_id":"00000000-0000-0000-0000-000000000001","task":"continue"}"#,
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            assignment.to_agent_id.as_str(),
            "00000000-0000-0000-0000-000000000001"
        );
    }

    #[test]
    fn host_assignment_ignores_non_assignment_protocol_payloads() {
        let parsed = HostAssignment::parse(
            r#"{"protocol":"va-agent-protocol","kind":"report","from_agent_id":"a"}"#,
        )
        .unwrap();

        assert!(parsed.is_none());
    }
}
