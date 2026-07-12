//! Bridge handler for spawned subagents.
//!
//! Subagent output is limited to rich presentation channels. The host thread
//! remains the only visible participant in IM/stdio channels; subagents stream
//! into web/tui surfaces. A subagent permission request returns to the host
//! target that triggered that subagent turn.

use std::sync::{Arc, Weak};

use agent_client_protocol::schema::v1 as acp;
use tokio::sync::Mutex;

use crate::agent::AgentClientHandler;
use crate::routing::{channel_traits, ActiveTurnTarget, RouteKey};
use crate::workspace::threads::runtime::{SubagentCompletionResult, SubagentCompletionValidator};
use crate::workspace::threads::{ThreadAgent, ThreadAgentStatus, WorkspaceThreadId};
use crate::workspace::WorkspaceThreadManager;

use super::agent_protocol::{
    notification_payload, notification_payload_with_text, session_update_text,
    synthetic_agent_message_payload, AgentProtocolFilter,
};
use super::plugin_host::PluginHost;
use super::types::ChannelOutput;

pub struct SubagentBridgeHandler {
    plugin_host: Arc<PluginHost>,
    workspace_threads: Weak<WorkspaceThreadManager>,
    thread_id: WorkspaceThreadId,
    thread_agent: ThreadAgent,
    active_turn_target: ActiveTurnTarget,
    report_tracker: Arc<SubagentReportTracker>,
}

impl SubagentBridgeHandler {
    pub fn for_thread(
        plugin_host: Arc<PluginHost>,
        workspace_threads: &Arc<WorkspaceThreadManager>,
        thread_id: WorkspaceThreadId,
        thread_agent: ThreadAgent,
        active_turn_target: ActiveTurnTarget,
        report_tracker: Arc<SubagentReportTracker>,
    ) -> Self {
        Self {
            plugin_host,
            workspace_threads: Arc::downgrade(workspace_threads),
            thread_id,
            thread_agent,
            active_turn_target,
            report_tracker,
        }
    }

    async fn attached_rich_agent_event_routes(&self) -> Vec<RouteKey> {
        let Some(workspace_threads) = self.workspace_threads.upgrade() else {
            return Vec::new();
        };
        match workspace_threads
            .attached_routes_for_thread(&self.thread_id)
            .await
        {
            Ok(routes) => routes
                .into_iter()
                .filter(|route| channel_traits(&route.channel_kind).rich_agent_events)
                .collect(),
            Err(error) => {
                tracing::warn!(
                    thread_id = %self.thread_id,
                    error = %error,
                    "failed to resolve presentation routes for subagent output"
                );
                Vec::new()
            }
        }
    }
}

#[async_trait::async_trait]
impl AgentClientHandler for SubagentBridgeHandler {
    async fn session_notification(&self, args: acp::SessionNotification) -> acp::Result<()> {
        let Some(payload) = self.report_tracker.record_notification(&args).await? else {
            return Ok(());
        };

        for route in self.attached_rich_agent_event_routes().await {
            self.plugin_host.send_output(ChannelOutput::SubagentAcp {
                route,
                agent: self.thread_agent.clone(),
                payload: payload.clone(),
            });
        }
        Ok(())
    }

    async fn prompt_finished(&self, success: bool) -> acp::Result<()> {
        let (session_id, visible_tail) = self.report_tracker.finish_stream(success).await;
        let Some(session_id) = session_id else {
            return Ok(());
        };
        if visible_tail.is_empty() {
            return Ok(());
        }
        let payload = synthetic_agent_message_payload(&session_id, visible_tail);
        for route in self.attached_rich_agent_event_routes().await {
            self.plugin_host.send_output(ChannelOutput::SubagentAcp {
                route,
                agent: self.thread_agent.clone(),
                payload: payload.clone(),
            });
        }
        Ok(())
    }

    async fn request_permission(
        &self,
        args: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        super::permission::forward_permission_request(
            &self.plugin_host,
            &self.active_turn_target,
            args,
            Some((
                "subagent",
                serde_json::json!({
                    "id": self.thread_agent.id.to_string(),
                    "name": self.thread_agent.name.clone(),
                    "turn_id": self.thread_agent.turn_id.to_string(),
                }),
            )),
        )
        .await
    }
}

pub struct SubagentReportTracker {
    expected_agent: ThreadAgent,
    state: Mutex<SubagentReportState>,
}

#[derive(Default)]
struct SubagentReportState {
    filter: AgentProtocolFilter,
    last_session_id: Option<String>,
    report_frame: Option<Result<String, String>>,
}

impl SubagentReportTracker {
    pub fn new(expected_agent: ThreadAgent) -> Self {
        Self {
            expected_agent,
            state: Mutex::new(SubagentReportState::default()),
        }
    }

    async fn record_notification(
        &self,
        args: &acp::SessionNotification,
    ) -> acp::Result<Option<serde_json::Value>> {
        if matches!(&args.update, acp::SessionUpdate::UserMessageChunk(_)) {
            return Ok(None);
        }
        let Some(text) = session_update_text(&args.update) else {
            return notification_payload(args).map(Some);
        };
        let visible = {
            let mut state = self.state.lock().await;
            state.last_session_id = Some(args.session_id.to_string());
            state.filter.feed_text(text)
        };
        if visible.is_empty() {
            return Ok(None);
        }
        notification_payload_with_text(args, visible).map(Some)
    }

    async fn finish_stream(&self, success: bool) -> (Option<String>, String) {
        let mut state = self.state.lock().await;
        let session_id = state.last_session_id.clone();
        let finished = state.filter.finish();
        if success {
            state.report_frame = finished.frame;
        } else {
            state.report_frame = None;
        }
        (session_id, finished.visible_tail)
    }
}

#[async_trait::async_trait]
impl SubagentCompletionValidator for SubagentReportTracker {
    async fn reset_completion(&self) {
        *self.state.lock().await = SubagentReportState::default();
    }

    async fn validate_completion(&self) -> Result<SubagentCompletionResult, String> {
        let frame = self.state.lock().await.report_frame.clone();
        let envelope = match frame {
            Some(Ok(envelope)) => envelope,
            Some(Err(error)) => return Err(error),
            None => {
                return Err("subagent final response missing va-agent-protocol report".to_string())
            }
        };
        validate_report_envelope(&self.expected_agent, &envelope)
    }
}

#[cfg(test)]
fn validate_report_text(
    expected_agent: &ThreadAgent,
    text: &str,
) -> Result<SubagentCompletionResult, String> {
    let envelope = extract_single_protocol_envelope(text)?;
    validate_report_envelope(expected_agent, envelope)
}

fn validate_report_envelope(
    expected_agent: &ThreadAgent,
    envelope: &str,
) -> Result<SubagentCompletionResult, String> {
    let value: serde_json::Value = serde_json::from_str(envelope)
        .map_err(|error| format!("invalid va-agent-protocol report JSON: {}", error))?;
    let object = value
        .as_object()
        .ok_or_else(|| "va-agent-protocol report must be a JSON object".to_string())?;

    require_field(object, "protocol", "va-agent-protocol")?;
    require_field(object, "kind", "report")?;
    require_field(object, "turn_id", expected_agent.turn_id.as_str())?;
    require_field(object, "from_agent_id", expected_agent.id.as_str())?;

    let status = object
        .get("status")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "va-agent-protocol report missing string field `status`".to_string())?;
    let summary = object
        .get("summary")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|summary| !summary.is_empty())
        .ok_or_else(|| "va-agent-protocol report missing non-empty `summary`".to_string())?;

    match status {
        "completed" => Ok(SubagentCompletionResult {
            status: ThreadAgentStatus::Completed,
            last_error: None,
            report: Some(value),
        }),
        "error" => Ok(SubagentCompletionResult {
            status: ThreadAgentStatus::Error,
            last_error: Some(summary.to_string()),
            report: Some(value),
        }),
        other => Err(format!(
            "va-agent-protocol report has invalid status `{}`; expected `completed` or `error`",
            other
        )),
    }
}

#[cfg(test)]
fn extract_single_protocol_envelope(text: &str) -> Result<&str, String> {
    const OPEN: &str = "<va-agent-protocol>";
    const CLOSE: &str = "</va-agent-protocol>";
    let trimmed = text.trim();
    if trimmed.matches(OPEN).count() != 1 || trimmed.matches(CLOSE).count() != 1 {
        return Err("subagent final response must contain exactly one envelope".to_string());
    }
    let start = trimmed.find(OPEN).ok_or_else(|| {
        "subagent final response missing <va-agent-protocol> envelope".to_string()
    })?;
    let content_start = start + OPEN.len();
    let close_offset = trimmed[content_start..].find(CLOSE).ok_or_else(|| {
        "subagent final response missing </va-agent-protocol> close tag".to_string()
    })?;
    let content_end = content_start + close_offset;
    let after = content_end + CLOSE.len();
    if !trimmed[after..].trim().is_empty() {
        return Err(
            "subagent final response must not include prose after va-agent-protocol envelope"
                .to_string(),
        );
    }
    Ok(trimmed[content_start..content_end].trim())
}

fn require_field(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
    expected: &str,
) -> Result<(), String> {
    let actual = object
        .get(field)
        .and_then(|value| value.as_str())
        .ok_or_else(|| format!("va-agent-protocol report missing string field `{}`", field))?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "va-agent-protocol report field `{}` expected `{}`, got `{}`",
            field, expected, actual
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::{ActiveTurnTarget, ChannelTarget, RouteKey};
    use crate::workspace::threads::{MultiAgentTurnId, ThreadAgentId};

    fn test_agent() -> ThreadAgent {
        ThreadAgent::ready(
            ThreadAgentId::from("00000000-0000-0000-0000-000000000001"),
            MultiAgentTurnId::from("mat_a"),
            "John Planner",
            "codex",
            None,
            "va/subagents/mat_a/john-planner",
            "/tmp/john-planner",
            Some("plan".to_string()),
        )
    }

    fn permission_handler(
        plugin_host: Arc<PluginHost>,
        active_turn_target: ActiveTurnTarget,
    ) -> SubagentBridgeHandler {
        let agent = test_agent();
        SubagentBridgeHandler {
            plugin_host,
            workspace_threads: Weak::new(),
            thread_id: WorkspaceThreadId::from("thread-1"),
            thread_agent: agent.clone(),
            active_turn_target,
            report_tracker: Arc::new(SubagentReportTracker::new(agent)),
        }
    }

    #[tokio::test]
    async fn subagent_permission_uses_the_host_turn_target() {
        let (input_tx, _input_rx) = tokio::sync::mpsc::unbounded_channel();
        let plugin_host = Arc::new(PluginHost::new(input_tx));
        let (output_tx, mut output_rx) = tokio::sync::mpsc::unbounded_channel();
        plugin_host.register_websocket_plugin("web", output_tx);
        let active_turn_target = ActiveTurnTarget::default();
        let target = ChannelTarget::new(
            RouteKey::new("web", "chat-1"),
            Some("message-1".to_string()),
        );
        let _target_guard = active_turn_target.install(target.clone());
        let handler = Arc::new(permission_handler(
            Arc::clone(&plugin_host),
            active_turn_target,
        ));

        let permission_task = tokio::spawn({
            let handler = Arc::clone(&handler);
            async move {
                handler
                    .request_permission(super::super::permission::test_permission_request())
                    .await
            }
        });
        let output = output_rx.recv().await.expect("permission output");
        let ChannelOutput::PermissionRequest {
            route,
            reply_to,
            request_id,
            payload,
        } = output
        else {
            panic!("expected permission request");
        };
        assert_eq!(route, target.route);
        assert_eq!(reply_to, target.reply_to);
        assert_eq!(payload["subagent"]["id"], self::test_agent().id.to_string());

        let response =
            acp::RequestPermissionResponse::new(acp::RequestPermissionOutcome::Cancelled);
        plugin_host
            .respond_permission("web", &request_id, response.clone())
            .unwrap();
        assert_eq!(permission_task.await.unwrap().unwrap(), response);
    }

    #[tokio::test]
    async fn cancelling_subagent_turn_clears_permission_and_rejects_late_response() {
        let (input_tx, _input_rx) = tokio::sync::mpsc::unbounded_channel();
        let plugin_host = Arc::new(PluginHost::new(input_tx));
        let (output_tx, mut output_rx) = tokio::sync::mpsc::unbounded_channel();
        plugin_host.register_websocket_plugin("web", output_tx);
        let active_turn_target = ActiveTurnTarget::default();
        let _target_guard =
            active_turn_target.install(ChannelTarget::for_route(RouteKey::new("web", "chat-1")));
        let handler = Arc::new(permission_handler(
            Arc::clone(&plugin_host),
            active_turn_target.clone(),
        ));

        let permission_task = tokio::spawn({
            let handler = Arc::clone(&handler);
            async move {
                handler
                    .request_permission(super::super::permission::test_permission_request())
                    .await
            }
        });
        let output = output_rx.recv().await.expect("permission output");
        let ChannelOutput::PermissionRequest { request_id, .. } = output else {
            panic!("expected permission request");
        };

        active_turn_target.cancel_current();

        let response = permission_task.await.unwrap().unwrap();
        assert_eq!(response.outcome, acp::RequestPermissionOutcome::Cancelled);

        assert!(plugin_host
            .respond_permission(
                "web",
                &request_id,
                acp::RequestPermissionResponse::new(acp::RequestPermissionOutcome::Cancelled,),
            )
            .is_err());
    }

    #[test]
    fn validates_completed_report_envelope() {
        let text = r#"
Completed the task.

<va-agent-protocol>
{
  "protocol": "va-agent-protocol",
  "kind": "report",
  "turn_id": "mat_a",
  "from_agent_id": "00000000-0000-0000-0000-000000000001",
  "status": "completed",
  "summary": "Done."
}
</va-agent-protocol>
"#;

        let result = validate_report_text(&test_agent(), text).unwrap();

        assert_eq!(result.status, ThreadAgentStatus::Completed);
        assert!(result.last_error.is_none());
        assert!(result.report.is_some());
    }

    #[test]
    fn rejects_report_with_prose_after_envelope() {
        let text = r#"
<va-agent-protocol>
{"protocol":"va-agent-protocol","kind":"report","turn_id":"mat_a","from_agent_id":"00000000-0000-0000-0000-000000000001","status":"completed","summary":"Done."}
</va-agent-protocol>
Done.
"#;

        let error = validate_report_text(&test_agent(), text).unwrap_err();

        assert!(error.contains("after va-agent-protocol"));
    }

    #[test]
    fn report_status_error_maps_to_agent_error() {
        let text = r#"
<va-agent-protocol>
{"protocol":"va-agent-protocol","kind":"report","turn_id":"mat_a","from_agent_id":"00000000-0000-0000-0000-000000000001","status":"error","summary":"Blocked by failing test."}
</va-agent-protocol>
"#;

        let result = validate_report_text(&test_agent(), text).unwrap();

        assert_eq!(result.status, ThreadAgentStatus::Error);
        assert_eq!(
            result.last_error.as_deref(),
            Some("Blocked by failing test.")
        );
    }

    #[test]
    fn report_tracker_reset_supports_multiple_assignments() {
        let tracker = SubagentReportTracker::new(test_agent());
        let first = acp::SessionNotification::new(
            "session-a",
            acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
                acp::TextContent::new(
                    r#"<va-agent-protocol>{"protocol":"va-agent-protocol","kind":"report","turn_id":"mat_a","from_agent_id":"00000000-0000-0000-0000-000000000001","status":"completed","summary":"First."}</va-agent-protocol>"#,
                ),
            ))),
        );
        let second = acp::SessionNotification::new(
            "session-a",
            acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
                acp::TextContent::new(
                    r#"<va-agent-protocol>{"protocol":"va-agent-protocol","kind":"report","turn_id":"mat_a","from_agent_id":"00000000-0000-0000-0000-000000000001","status":"error","summary":"Second failed."}</va-agent-protocol>"#,
                ),
            ))),
        );

        futures::executor::block_on(async {
            let visible = tracker.record_notification(&first).await.unwrap();
            assert!(visible.is_none());
            tracker.finish_stream(true).await;
            assert_eq!(
                tracker.validate_completion().await.unwrap().status,
                ThreadAgentStatus::Completed
            );
            tracker.reset_completion().await;
            let visible = tracker.record_notification(&second).await.unwrap();
            assert!(visible.is_none());
            tracker.finish_stream(true).await;
            let result = tracker.validate_completion().await.unwrap();
            assert_eq!(result.status, ThreadAgentStatus::Error);
            assert_eq!(result.last_error.as_deref(), Some("Second failed."));
        });
    }
}
