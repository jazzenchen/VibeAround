use common::channels::types::{ChannelSessionStart, ThreadReplyPayload};
use common::channels::ChannelOutput;

use crate::api_types::ChatEvent;

/// Translate a `ChannelOutput` into a wire `ChatEvent`.
pub(super) fn output_to_chat_event(output: ChannelOutput) -> ChatEvent {
    match output {
        ChannelOutput::ThreadReply { reply, .. } => match reply.payload {
            ThreadReplyPayload::AcpSessionNotification { notification } => {
                acp_passthrough(notification)
            }
        },
        ChannelOutput::RawAcp { payload, .. } => acp_passthrough(payload),
        ChannelOutput::SystemText { text, .. } => ChatEvent::SystemText { text },
        ChannelOutput::AgentReady { agent, version, .. } => {
            ChatEvent::AgentReady { agent, version }
        }
        ChannelOutput::SessionReady { session_id, .. } => ChatEvent::SessionReady { session_id },
        ChannelOutput::SessionInfo { info, .. } => ChatEvent::SystemText {
            text: format!(
                "Workspace: {}\nAgent: {}{}\nProfile: {}\n{}: {}",
                info.workspace_path,
                info.agent.name,
                if info.agent.version.is_empty() {
                    String::new()
                } else {
                    format!(" v{}", info.agent.version)
                },
                info.agent
                    .profile_id
                    .unwrap_or_else(|| "Native".to_string()),
                match info.start {
                    ChannelSessionStart::New => "New session started",
                    ChannelSessionStart::Resumed => "Continuing from session",
                },
                info.session_id
            ),
        },
        ChannelOutput::SessionMode { session_mode, .. } => ChatEvent::SessionMode { session_mode },
        ChannelOutput::CommandMenu {
            system_commands,
            agent_commands,
            ..
        } => ChatEvent::CommandMenu {
            system_commands,
            agent_commands,
        },
        ChannelOutput::PermissionRequest {
            request_id,
            payload,
            ..
        } => ChatEvent::PermissionRequest {
            request_id,
            request: payload,
        },
        ChannelOutput::MultiAgentTurn { turn, agents, .. } => {
            ChatEvent::MultiAgentTurn { turn, agents }
        }
        ChannelOutput::SubagentStatus { agent, .. } => ChatEvent::SubagentStatus { agent },
        ChannelOutput::SubagentAcp { agent, payload, .. } => {
            ChatEvent::SubagentAcpNotification { agent, payload }
        }
        ChannelOutput::TurnStatus { active, .. } => ChatEvent::TurnStatus { active },
    }
}

/// Pass ACP session notifications through as `AcpNotification`.
fn acp_passthrough(payload: serde_json::Value) -> ChatEvent {
    ChatEvent::AcpNotification { payload }
}

pub(super) fn permission_response_error_event(request_id: &str, error: &str) -> ChatEvent {
    ChatEvent::Error {
        error: format!("Permission response for request `{request_id}` was ignored: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_response_error_is_user_visible() {
        let ChatEvent::Error { error } = super::permission_response_error_event(
            "req-1",
            "permission request is no longer pending",
        ) else {
            panic!("expected error event");
        };

        assert!(error.contains("req-1"));
        assert!(error.contains("permission request is no longer pending"));
    }
}
