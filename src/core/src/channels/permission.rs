use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;

use crate::routing::ActiveTurnTarget;

use super::plugin_host::{PendingPermissionRegistration, PluginHost};
use super::types::ChannelOutput;

pub(super) async fn forward_permission_request(
    plugin_host: &Arc<PluginHost>,
    active_turn_target: &ActiveTurnTarget,
    args: acp::RequestPermissionRequest,
    extra_payload: Option<(&str, serde_json::Value)>,
) -> acp::Result<acp::RequestPermissionResponse> {
    if args.options.is_empty() {
        return Err(acp::Error::method_not_found());
    }

    let Some((target, mut turn_cancelled)) = active_turn_target.current_with_cancellation() else {
        return Ok(cancelled_response());
    };

    let request_id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let _registration = PendingPermissionRegistration::register(
        plugin_host,
        request_id.clone(),
        target.route.channel_instance_id().to_string(),
        tx,
    );
    let mut payload = serde_json::to_value(&args).map_err(|error| {
        acp::Error::new(-32603, format!("serialize requestPermission: {error}"))
    })?;
    if let (Some(payload), Some((key, value))) = (payload.as_object_mut(), extra_payload) {
        payload.insert(key.to_string(), value);
    }

    plugin_host.send_output(ChannelOutput::PermissionRequest {
        route: target.route,
        reply_to: target.reply_to,
        request_id,
        payload,
    });

    tokio::select! {
        biased;
        _ = turn_cancelled.wait_for(|cancelled| *cancelled) => Ok(cancelled_response()),
        response = rx => Ok(response.unwrap_or_else(|_| cancelled_response())),
    }
}

fn cancelled_response() -> acp::RequestPermissionResponse {
    acp::RequestPermissionResponse::new(acp::RequestPermissionOutcome::Cancelled)
}

#[cfg(test)]
pub(super) fn test_permission_request() -> acp::RequestPermissionRequest {
    acp::RequestPermissionRequest::new(
        "session-1",
        acp::ToolCallUpdate::new("tool-1", acp::ToolCallUpdateFields::new()),
        vec![acp::PermissionOption::new(
            "allow-once",
            "Allow once",
            acp::PermissionOptionKind::AllowOnce,
        )],
    )
}
