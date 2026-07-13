//! `ChannelOutput` → plugin dispatch.
//!
//! Each variant of `ChannelOutput` maps to a different ACP Client call:
//!
//! - `ThreadReply`        → `ext_notification("va/thread_reply", ...)`
//! - `SystemText`         → `ext_notification("va/system_text", ...)`
//! - `AgentReady`         → `ext_notification("va/agent_ready", ...)`
//! - `SessionReady`       → `ext_notification("va/session_ready", ...)`
//! - `SessionInfo`        → `ext_notification("va/session_info", ...)`
//! - `CommandMenu`        → `ext_notification("va/command_menu", ...)`
//! - `PromptDone`         → no-op for stdio plugins (their `prompt()` call already resolves)
//! - `PermissionRequest`  → real `request_permission` call; response is
//!   routed back through `PluginHost::pending_permissions`.
//! - `MultiAgentTurn`     → web-only for now; stdio/IM plugins do not see it.
//! - `Subagent*`          → web-only for now; stdio/IM plugins do not see it.

use std::sync::Arc;

use serde_json::value::RawValue;

use acp::schema::v1 as schema;
use agent_client_protocol as acp;

use super::super::plugin_host::PluginHost;
use super::super::ChannelOutput;

/// Forward a `ChannelOutput` to the plugin via the ACP Client API.
pub(super) async fn forward_output_to_plugin(
    conn: &acp::ConnectionTo<acp::Client>,
    channel_kind: &str,
    plugin_host: &Arc<PluginHost>,
    output: ChannelOutput,
) -> Result<(), String> {
    match output {
        ChannelOutput::ThreadReply {
            route,
            reply_to,
            reply,
        } => {
            let target = route_target(&route, reply_to.as_deref());
            send_ext_notification(
                conn,
                channel_kind,
                "va/thread_reply",
                &serde_json::json!({
                "target": target,
                "reply": reply,
                }),
            )
            .await?;
        }
        ChannelOutput::RawAcp { route, .. } => {
            tracing::info!(
                "[{}] dropping RawAcp for stdio route={} because agent responses now use ThreadReply",
                channel_kind,
                route
            );
        }
        ChannelOutput::SystemText {
            route,
            text,
            reply_to,
        } => {
            let target = route_target(&route, reply_to.as_deref());
            send_ext_notification(
                conn,
                channel_kind,
                "va/system_text",
                &serde_json::json!({
                    "chatId": route.chat_id,
                    "target": target,
                    "text": text,
                }),
            )
            .await?;
        }
        ChannelOutput::AgentReady {
            route,
            reply_to,
            agent,
            version,
        } => {
            let target = route_target(&route, reply_to.as_deref());
            send_ext_notification(
                conn,
                channel_kind,
                "va/agent_ready",
                &serde_json::json!({
                    "chatId": route.chat_id,
                    "target": target,
                    "agent": agent,
                    "version": version,
                }),
            )
            .await?;
        }
        ChannelOutput::SessionReady {
            route,
            reply_to,
            session_id,
        } => {
            let target = route_target(&route, reply_to.as_deref());
            send_ext_notification(
                conn,
                channel_kind,
                "va/session_ready",
                &serde_json::json!({
                    "chatId": route.chat_id,
                    "target": target,
                    "sessionId": session_id,
                }),
            )
            .await?;
        }
        ChannelOutput::SessionInfo {
            route,
            reply_to,
            info,
        } => {
            let target = route_target(&route, reply_to.as_deref());
            send_ext_notification(
                conn,
                channel_kind,
                "va/session_info",
                &serde_json::json!({
                    "chatId": route.chat_id,
                    "target": target,
                    "info": info,
                }),
            )
            .await?;
        }
        ChannelOutput::SessionMode {
            route,
            session_mode,
        } => {
            let target = route_target(&route, None);
            send_ext_notification(
                conn,
                channel_kind,
                "va/session_mode",
                &serde_json::json!({
                    "chatId": route.chat_id,
                    "target": target,
                    "sessionMode": session_mode,
                }),
            )
            .await?;
        }
        ChannelOutput::CommandMenu {
            route,
            system_commands,
            agent_commands,
        } => {
            let target = route_target(&route, None);
            send_ext_notification(
                conn,
                channel_kind,
                "va/command_menu",
                &serde_json::json!({
                    "chatId": route.chat_id,
                    "target": target,
                    "systemCommands": system_commands,
                    "agentCommands": agent_commands,
                }),
            )
            .await?;
        }
        ChannelOutput::PromptDone { .. }
        | ChannelOutput::TurnStatus { .. }
        | ChannelOutput::MultiAgentTurn { .. }
        | ChannelOutput::SubagentStatus { .. }
        | ChannelOutput::SubagentAcp { .. } => {}
        ChannelOutput::PermissionRequest {
            route,
            reply_to,
            request_id,
            payload,
        } => {
            let channel_instance_id = route.channel_instance_id().to_string();
            // Forward as a VibeAround ext method so the transport envelope can
            // carry the IM chat target while the ACP request keeps its real
            // agent sessionId.
            let request: schema::RequestPermissionRequest =
                match serde_json::from_value(payload) {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::info!(
                        "[{}] failed to parse PermissionRequest payload route={} request_id={}: {}",
                        channel_kind, route, request_id, e
                    );
                        complete_permission_request(
                            plugin_host,
                            &channel_instance_id,
                            &route.to_string(),
                            &request_id,
                            schema::RequestPermissionResponse::new(
                                schema::RequestPermissionOutcome::Cancelled,
                            ),
                        )
                        .await;
                        return Ok(());
                    }
                };
            let params = serde_json::json!({
                "target": route_target(&route, reply_to.as_deref()),
                "requestId": request_id.clone(),
                "request": request,
            });
            let Some(raw_params) = raw_json_params(channel_kind, &params) else {
                complete_permission_request(
                    plugin_host,
                    &channel_instance_id,
                    &route.to_string(),
                    &request_id,
                    schema::RequestPermissionResponse::new(
                        schema::RequestPermissionOutcome::Cancelled,
                    ),
                )
                .await;
                return Ok(());
            };
            let response = conn
                .send_request(schema::AgentRequest::ExtMethodRequest(
                    schema::ExtRequest::new("_va/request_permission", raw_params),
                ))
                .block_task()
                .await;
            match response {
                Ok(value) => {
                    match serde_json::from_value::<schema::RequestPermissionResponse>(value) {
                        Ok(resp) => {
                            complete_permission_request(
                                plugin_host,
                                &channel_instance_id,
                                &route.to_string(),
                                &request_id,
                                resp,
                            )
                            .await;
                        }
                        Err(e) => {
                            tracing::info!(
                                "[{}] plugin requestPermission returned invalid response route={} request_id={}: {}",
                                channel_kind,
                                route,
                                request_id,
                                e
                            );
                            complete_permission_request(
                                plugin_host,
                                &channel_instance_id,
                                &route.to_string(),
                                &request_id,
                                schema::RequestPermissionResponse::new(
                                    schema::RequestPermissionOutcome::Cancelled,
                                ),
                            )
                            .await;
                        }
                    }
                }
                Err(e) => {
                    let error = e.to_string();
                    tracing::info!(
                        "[{}] plugin requestPermission failed route={} request_id={}: {}",
                        channel_kind,
                        route,
                        request_id,
                        e
                    );
                    complete_permission_request(
                        plugin_host,
                        &channel_instance_id,
                        &route.to_string(),
                        &request_id,
                        schema::RequestPermissionResponse::new(
                            schema::RequestPermissionOutcome::Cancelled,
                        ),
                    )
                    .await;
                    return Err(error);
                }
            }
        }
    }
    Ok(())
}

fn route_target(route: &crate::routing::RouteKey, reply_to: Option<&str>) -> serde_json::Value {
    let mut target = serde_json::Map::from_iter([
        (
            "channelInstanceId".to_string(),
            route.channel_instance_id().into(),
        ),
        (
            "actorId".to_string(),
            route
                .actor_id()
                .unwrap_or_else(|| route.channel_instance_id())
                .into(),
        ),
        ("chatId".to_string(), route.chat_id.clone().into()),
    ]);
    if let Some(topic_id) = route.topic_id() {
        target.insert("topicId".to_string(), topic_id.into());
    }
    if let Some(reply_to) = reply_to.filter(|reply_to| !reply_to.is_empty()) {
        target.insert("replyTo".to_string(), reply_to.into());
    }
    serde_json::Value::Object(target)
}

async fn complete_permission_request(
    plugin_host: &PluginHost,
    channel_instance_id: &str,
    route: &str,
    request_id: &str,
    response: schema::RequestPermissionResponse,
) {
    if let Err(error) = plugin_host
        .respond_permission(channel_instance_id, request_id, response)
        .await
    {
        tracing::info!(
            "[{}] PermissionRequest response dropped route={} request_id={}: {}",
            channel_instance_id,
            route,
            request_id,
            error
        );
    }
}

fn raw_json_params(channel_kind: &str, params: &serde_json::Value) -> Option<Arc<RawValue>> {
    match RawValue::from_string(serde_json::to_string(params).unwrap_or_default()) {
        Ok(raw) => Some(Arc::from(raw)),
        Err(error) => {
            tracing::info!(
                "[{}] failed to serialize ext params: {}",
                channel_kind,
                error
            );
            None
        }
    }
}

async fn send_ext_notification(
    conn: &acp::ConnectionTo<acp::Client>,
    channel_kind: &str,
    method: &str,
    params: &serde_json::Value,
) -> Result<(), String> {
    let Some(raw_params) = raw_json_params(channel_kind, params) else {
        return Err(format!(
            "failed to serialize ext_notification {method} params"
        ));
    };
    let notification = schema::AgentNotification::ExtNotification(schema::ExtNotification::new(
        format!("_{}", method),
        raw_params,
    ));
    if let Err(error) = conn.send_notification(notification) {
        let message = error.to_string();
        tracing::info!(
            "[{}] failed to send ext_notification {}: {}",
            channel_kind,
            method,
            error
        );
        return Err(message);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::route_target;
    use crate::routing::RouteKey;

    #[test]
    fn extended_output_target_preserves_actor_and_topic() {
        let route = RouteKey::with_actor(
            "feishu",
            "feishu-primary",
            "chat-1",
            "codex-reviewer",
            Some("topic-1".to_string()),
        );

        assert_eq!(
            route_target(&route, None),
            serde_json::json!({
                "channelInstanceId": "feishu-primary",
                "actorId": "codex-reviewer",
                "chatId": "chat-1",
                "topicId": "topic-1"
            })
        );
        assert_eq!(
            route_target(&route, Some("message-1"))["replyTo"],
            "message-1"
        );
    }
}
