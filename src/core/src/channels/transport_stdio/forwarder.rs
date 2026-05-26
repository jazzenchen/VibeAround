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

use acp::schema;
use agent_client_protocol as acp;

use super::super::plugin_host::PluginHost;
use super::super::ChannelOutput;

/// Forward a `ChannelOutput` to the plugin via the ACP Client API.
pub(super) async fn forward_output_to_plugin(
    conn: &acp::ConnectionTo<acp::Client>,
    channel_kind: &str,
    plugin_host: &Arc<PluginHost>,
    output: ChannelOutput,
) {
    match output {
        ChannelOutput::ThreadReply { route, reply } => {
            send_ext_notification(
                conn,
                channel_kind,
                "va/thread_reply",
                &serde_json::json!({
                    "target": {
                        "chatId": route.chat_id.clone(),
                    },
                    "reply": reply,
                }),
            )
            .await;
        }
        ChannelOutput::RawAcp { route, .. } => {
            tracing::info!(
                "[{}] dropping RawAcp for stdio route={} because agent responses now use ThreadReply",
                channel_kind,
                route
            );
        }
        ChannelOutput::SystemText { route, text, .. } => {
            send_ext_notification(
                conn,
                channel_kind,
                "va/system_text",
                &serde_json::json!({
                    "chatId": route.chat_id,
                    "text": text,
                }),
            )
            .await;
        }
        ChannelOutput::AgentReady {
            route,
            agent,
            version,
            ..
        } => {
            send_ext_notification(
                conn,
                channel_kind,
                "va/agent_ready",
                &serde_json::json!({
                    "chatId": route.chat_id,
                    "agent": agent,
                    "version": version,
                }),
            )
            .await;
        }
        ChannelOutput::SessionReady {
            route, session_id, ..
        } => {
            send_ext_notification(
                conn,
                channel_kind,
                "va/session_ready",
                &serde_json::json!({
                    "chatId": route.chat_id,
                    "sessionId": session_id,
                }),
            )
            .await;
        }
        ChannelOutput::SessionInfo { route, info } => {
            send_ext_notification(
                conn,
                channel_kind,
                "va/session_info",
                &serde_json::json!({
                    "chatId": route.chat_id,
                    "info": info,
                }),
            )
            .await;
        }
        ChannelOutput::SessionMode {
            route,
            session_mode,
        } => {
            send_ext_notification(
                conn,
                channel_kind,
                "va/session_mode",
                &serde_json::json!({
                    "chatId": route.chat_id,
                    "sessionMode": session_mode,
                }),
            )
            .await;
        }
        ChannelOutput::CommandMenu {
            route,
            system_commands,
            agent_commands,
        } => {
            send_ext_notification(
                conn,
                channel_kind,
                "va/command_menu",
                &serde_json::json!({
                    "chatId": route.chat_id,
                    "systemCommands": system_commands,
                    "agentCommands": agent_commands,
                }),
            )
            .await;
        }
        ChannelOutput::PromptDone { .. }
        | ChannelOutput::TurnStatus { .. }
        | ChannelOutput::MultiAgentTurn { .. }
        | ChannelOutput::SubagentStatus { .. }
        | ChannelOutput::SubagentAcp { .. } => {}
        ChannelOutput::PermissionRequest {
            route,
            request_id,
            payload,
        } => {
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
                        if let Some((_, (_, tx))) =
                            plugin_host.pending_permissions.remove(&request_id)
                        {
                            let _ = tx.send(schema::RequestPermissionResponse::new(
                                schema::RequestPermissionOutcome::Cancelled,
                            ));
                        }
                        return;
                    }
                };
            let params = serde_json::json!({
                "target": {
                    "chatId": route.chat_id.clone(),
                },
                "requestId": request_id.clone(),
                "request": request,
            });
            let Some(raw_params) = raw_json_params(channel_kind, &params) else {
                if let Some((_, (_, tx))) = plugin_host.pending_permissions.remove(&request_id) {
                    let _ = tx.send(schema::RequestPermissionResponse::new(
                        schema::RequestPermissionOutcome::Cancelled,
                    ));
                }
                return;
            };
            let response = conn
                .send_request(schema::AgentRequest::ExtMethodRequest(
                    schema::ExtRequest::new("_va/request_permission", raw_params),
                ))
                .block_task()
                .await;
            let Some((_, (_, tx))) = plugin_host.pending_permissions.remove(&request_id) else {
                tracing::info!(
                    "[{}] PermissionRequest response dropped — no pending route={} request_id={}",
                    channel_kind,
                    route,
                    request_id
                );
                return;
            };
            match response {
                Ok(value) => {
                    match serde_json::from_value::<schema::RequestPermissionResponse>(value) {
                        Ok(resp) => {
                            let _ = tx.send(resp);
                        }
                        Err(e) => {
                            tracing::info!(
                                "[{}] plugin requestPermission returned invalid response route={} request_id={}: {}",
                                channel_kind,
                                route,
                                request_id,
                                e
                            );
                            let _ = tx.send(schema::RequestPermissionResponse::new(
                                schema::RequestPermissionOutcome::Cancelled,
                            ));
                        }
                    }
                }
                Err(e) => {
                    tracing::info!(
                        "[{}] plugin requestPermission failed route={} request_id={}: {}",
                        channel_kind,
                        route,
                        request_id,
                        e
                    );
                    let _ = tx.send(schema::RequestPermissionResponse::new(
                        schema::RequestPermissionOutcome::Cancelled,
                    ));
                }
            }
        }
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
) {
    let Some(raw_params) = raw_json_params(channel_kind, params) else {
        return;
    };
    let notification = schema::AgentNotification::ExtNotification(schema::ExtNotification::new(
        format!("_{}", method),
        raw_params,
    ));
    if let Err(error) = conn.send_notification(notification) {
        tracing::info!(
            "[{}] failed to send ext_notification {}: {}",
            channel_kind,
            method,
            error
        );
    }
}
