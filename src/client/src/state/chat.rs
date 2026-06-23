use crate::events::ChatEvent;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChatState {
    pub channel_id: Option<String>,
    pub agents: Vec<crate::runtime::AgentInfo>,
    pub default_agent: Option<String>,
    pub current_agent: Option<ChatAgentInfo>,
    pub session_id: Option<String>,
    pub turn_active: bool,
    pub pending_permission_request_id: Option<String>,
    pub last_error: Option<String>,
    pub system_messages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatAgentInfo {
    pub agent: String,
    pub version: String,
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
            ChatEvent::PermissionRequest { request_id, .. } => {
                self.pending_permission_request_id = Some(request_id);
            }
            ChatEvent::PromptDone { .. } => {
                self.turn_active = false;
                self.pending_permission_request_id = None;
            }
            ChatEvent::TurnStatus { active } => {
                self.turn_active = active;
            }
            ChatEvent::SystemText { text } => {
                self.system_messages.push(text);
            }
            ChatEvent::Error { error } => {
                self.last_error = Some(error);
                self.turn_active = false;
            }
            ChatEvent::SessionMode { .. }
            | ChatEvent::CommandMenu { .. }
            | ChatEvent::MultiAgentTurn { .. }
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
        state.apply_event(ChatEvent::PromptDone { message_id: None });

        assert_eq!(state.channel_id.as_deref(), Some("web:1"));
        assert_eq!(state.default_agent.as_deref(), Some("codex"));
        assert_eq!(state.current_agent.as_ref().unwrap().agent, "Codex");
        assert_eq!(state.session_id.as_deref(), Some("s1"));
        assert!(!state.turn_active);
    }
}
