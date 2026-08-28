use crate::events::ChatEvent;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChatState {
    pub channel_id: Option<String>,
    pub agents: Vec<crate::runtime::AgentInfo>,
    pub default_agent: Option<String>,
    pub current_agent: Option<ChatAgentInfo>,
    pub session_id: Option<String>,
    pub session_mode: Option<serde_json::Value>,
    pub command_menu: Option<ChatCommandMenu>,
    pub multi_agent_turn: Option<ChatMultiAgentTurn>,
    pub turn_active: bool,
    pub pending_permission_request_id: Option<String>,
    pub pending_permission: Option<PendingPermission>,
    pub last_error: Option<String>,
    pub system_messages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatAgentInfo {
    pub agent: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatCommandMenu {
    pub system_commands: serde_json::Value,
    pub agent_commands: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatMultiAgentTurn {
    pub turn: serde_json::Value,
    pub agents: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingPermission {
    pub request_id: String,
    pub request: serde_json::Value,
}

impl ChatState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply_event(&mut self, event: ChatEvent) {
        match event {
            ChatEvent::Config {
                channel_id,
                agents,
                default_agent,
            } => {
                self.channel_id = Some(channel_id);
                self.agents = agents;
                self.default_agent = Some(default_agent);
            }
            ChatEvent::AgentReady { agent, version } => {
                self.current_agent = Some(ChatAgentInfo { agent, version });
            }
            ChatEvent::SessionReady { session_id } => {
                self.session_id = Some(session_id);
            }
            ChatEvent::SessionMode { session_mode } => {
                self.session_mode = Some(session_mode);
            }
            ChatEvent::CommandMenu {
                system_commands,
                agent_commands,
            } => {
                self.command_menu = Some(ChatCommandMenu {
                    system_commands,
                    agent_commands,
                });
            }
            ChatEvent::PermissionRequest {
                request_id,
                request,
            } => {
                self.pending_permission_request_id = Some(request_id.clone());
                self.pending_permission = Some(PendingPermission {
                    request_id,
                    request,
                });
            }
            ChatEvent::MultiAgentTurn { turn, agents } => {
                self.multi_agent_turn = Some(ChatMultiAgentTurn { turn, agents });
            }
            ChatEvent::TurnStatus { active } => {
                self.turn_active = active;
                if !active {
                    self.pending_permission_request_id = None;
                    self.pending_permission = None;
                }
            }
            ChatEvent::SystemText { text } => {
                self.system_messages.push(text);
            }
            ChatEvent::Error { error } => {
                self.last_error = Some(error);
                self.turn_active = false;
                self.pending_permission_request_id = None;
                self.pending_permission = None;
            }
            ChatEvent::SessionInfo { .. }
            | ChatEvent::PreviewRefresh
            | ChatEvent::ReplayStart { .. }
            | ChatEvent::ReplayDone { .. }
            | ChatEvent::SubagentStatus { .. }
            | ChatEvent::SubagentAcpNotification { .. }
            | ChatEvent::AcpNotification { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::events::ChatEvent;
    use crate::runtime::AgentInfo;
    use serde_json::json;

    use super::*;

    #[test]
    fn chat_state_reduces_lifecycle_events() {
        let mut state = ChatState::new();
        state.apply_event(ChatEvent::Config {
            channel_id: "web:1".into(),
            agents: vec![AgentInfo {
                id: "codex".into(),
                name: "Codex".into(),
                description: "Coding agent".into(),
            }],
            default_agent: "codex".into(),
        });
        state.apply_event(ChatEvent::AgentReady {
            agent: "Codex".into(),
            version: "1.0".into(),
        });
        state.apply_event(ChatEvent::SessionReady {
            session_id: "s1".into(),
        });
        state.apply_event(ChatEvent::TurnStatus { active: true });
        state.apply_event(ChatEvent::TurnStatus { active: false });

        assert_eq!(state.channel_id.as_deref(), Some("web:1"));
        assert_eq!(state.default_agent.as_deref(), Some("codex"));
        assert_eq!(state.current_agent.as_ref().unwrap().agent, "Codex");
        assert_eq!(state.session_id.as_deref(), Some("s1"));
        assert!(!state.turn_active);
    }

    #[test]
    fn chat_state_keeps_display_metadata() {
        let mut state = ChatState::new();
        state.apply_event(ChatEvent::SessionMode {
            session_mode: json!({ "currentModeId": "acceptEdits" }),
        });
        state.apply_event(ChatEvent::CommandMenu {
            system_commands: json!([{ "name": "/help" }]),
            agent_commands: json!([{ "name": "/review" }]),
        });
        state.apply_event(ChatEvent::MultiAgentTurn {
            turn: json!({ "id": "turn-1" }),
            agents: vec![json!({ "id": "agent-1" })],
        });

        assert_eq!(
            state.session_mode.as_ref().unwrap()["currentModeId"],
            "acceptEdits"
        );
        assert_eq!(
            state
                .command_menu
                .as_ref()
                .unwrap()
                .system_commands
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            state.multi_agent_turn.as_ref().unwrap().turn["id"],
            "turn-1"
        );
    }

    #[test]
    fn chat_state_keeps_and_clears_permission_payload() {
        let mut state = ChatState::new();
        state.apply_event(ChatEvent::PermissionRequest {
            request_id: "req-1".into(),
            request: json!({
                "toolCall": {
                    "title": "Read"
                }
            }),
        });

        assert_eq!(
            state.pending_permission_request_id.as_deref(),
            Some("req-1")
        );
        assert_eq!(
            state.pending_permission.as_ref().unwrap().request["toolCall"]["title"],
            "Read"
        );

        state.apply_event(ChatEvent::TurnStatus { active: true });

        assert_eq!(
            state.pending_permission_request_id.as_deref(),
            Some("req-1")
        );
        assert!(state.pending_permission.is_some());

        state.apply_event(ChatEvent::TurnStatus { active: false });

        assert_eq!(state.pending_permission_request_id, None);
        assert_eq!(state.pending_permission, None);
    }

    #[test]
    fn chat_state_clears_permission_on_error() {
        let mut state = ChatState::new();
        state.apply_event(ChatEvent::PermissionRequest {
            request_id: "req-1".into(),
            request: json!({ "toolCall": { "title": "Read" } }),
        });
        state.apply_event(ChatEvent::Error {
            error: "agent failed".into(),
        });

        assert_eq!(state.pending_permission_request_id, None);
        assert_eq!(state.pending_permission, None);
        assert!(!state.turn_active);
        assert_eq!(state.last_error.as_deref(), Some("agent failed"));
    }
}
