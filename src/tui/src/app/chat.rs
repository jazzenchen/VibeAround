use serde_json::Value;
use tokio::sync::mpsc;
use va_client::events::{ChatClientMessage, ChatEvent, ChatSessionAction};

use crate::app::TuiApp;
use crate::chat::{
    content_text, one_line, permission_prompt_text, tool_activity_text, ChatMessage, ChatRole,
};
use crate::chat_socket::ChatSocketEvent;
use crate::transport::HttpTransport;

impl TuiApp {
    pub(crate) async fn submit_chat_input(
        &mut self,
        transport: &HttpTransport,
        chat_tx: &mpsc::UnboundedSender<ChatClientMessage>,
    ) {
        let input = self.chat_input.trim().to_string();
        self.chat_input.clear();
        if input.is_empty() {
            return;
        }
        if input.starts_with('/') {
            self.run_slash_command(&input, transport, chat_tx).await;
            return;
        }

        self.chat_messages.push(ChatMessage {
            role: ChatRole::Request,
            text: input.clone(),
        });
        self.work_status = None;
        self.follow_chat_tail();
        self.send_chat_message(input, chat_tx);
    }

    async fn run_slash_command(
        &mut self,
        command: &str,
        transport: &HttpTransport,
        chat_tx: &mpsc::UnboundedSender<ChatClientMessage>,
    ) {
        let mut parts = command.split_whitespace();
        let name = parts.next().unwrap_or(command);
        match name {
            "/status" => self.open_status(transport).await,
            "/agent" => self.open_agent_picker(transport).await,
            "/help" => self.push_help_message(),
            "/clear" => {
                self.chat_messages.clear();
                self.follow_chat_tail();
            }
            "/back" => self.go_back(),
            "/stop" => self.send_chat_command(ChatClientMessage::stop(), chat_tx),
            "/allow" => {
                if let Some(option_id) = parts.next() {
                    if let Some(request_id) = self.chat_state.pending_permission_request_id.clone()
                    {
                        self.send_chat_command(
                            ChatClientMessage::permission_selected(request_id, option_id),
                            chat_tx,
                        );
                    } else {
                        self.push_notice("No pending permission request.");
                    }
                } else {
                    self.push_notice("Usage: /allow <option-id>");
                }
            }
            "/deny" | "/cancel" => {
                if let Some(request_id) = self.chat_state.pending_permission_request_id.clone() {
                    self.send_chat_command(
                        ChatClientMessage::permission_cancelled(request_id),
                        chat_tx,
                    );
                } else {
                    self.push_notice("No pending permission request.");
                }
            }
            unknown => self.chat_messages.push(ChatMessage {
                role: ChatRole::Notice,
                text: format!("Unknown command {unknown}. Try /status, /agent, /help, /clear."),
            }),
        }
    }

    fn push_help_message(&mut self) {
        self.chat_messages.push(ChatMessage {
            role: ChatRole::Notice,
            text: "/status runtime status  /agent agent context  /stop stop turn  /allow option-id  /deny  /clear clear chat".into(),
        });
    }

    fn push_notice(&mut self, text: impl Into<String>) {
        self.chat_messages.push(ChatMessage {
            role: ChatRole::Notice,
            text: text.into(),
        });
    }

    pub(crate) fn send_chat_message(
        &mut self,
        text: String,
        chat_tx: &mpsc::UnboundedSender<ChatClientMessage>,
    ) {
        let message = ChatClientMessage::Message {
            text,
            message_id: None,
            agent: self.effective_agent().map(str::to_string),
            profile_id: self.effective_profile().map(str::to_string),
            session_action: self.effective_session().map(|_| ChatSessionAction::Resume),
            session_id: self.effective_session().map(str::to_string),
            session_workspace: self.effective_workspace().map(str::to_string),
            permission_mode: None,
            attachments: Vec::new(),
        };
        self.send_chat_command(message, chat_tx);
    }

    fn send_chat_command(
        &mut self,
        message: ChatClientMessage,
        chat_tx: &mpsc::UnboundedSender<ChatClientMessage>,
    ) {
        if chat_tx.send(message).is_err() {
            self.last_error = Some("chat websocket task is not running".into());
        }
    }

    pub(crate) fn apply_chat_socket_event(&mut self, event: ChatSocketEvent) {
        match event {
            ChatSocketEvent::Connected => {
                self.chat_connected = true;
                self.last_error = None;
            }
            ChatSocketEvent::Closed => {
                self.chat_connected = false;
                self.push_notice("Chat websocket closed.");
            }
            ChatSocketEvent::Error(error) => {
                self.chat_connected = false;
                self.last_error = Some(error.clone());
                self.push_notice(format!("Chat websocket error: {error}"));
            }
            ChatSocketEvent::Event(event) => self.apply_chat_event(event),
        }
    }

    pub(crate) fn apply_chat_event(&mut self, event: ChatEvent) {
        match &event {
            ChatEvent::Config {
                default_agent,
                agents,
                ..
            } => {
                if self.selected_agent.is_none() {
                    self.selected_agent = Some(default_agent.clone());
                }
                self.agent_picker.agents = agents.clone();
            }
            ChatEvent::AgentReady { agent, version } => {
                self.last_action = Some(format!("agent {agent} {version} ready"));
            }
            ChatEvent::SessionReady { .. } => {}
            ChatEvent::SystemText { text } => {
                self.append_response_text(text);
            }
            ChatEvent::PermissionRequest {
                request_id,
                request,
            } => {
                self.work_status = None;
                self.push_notice(permission_prompt_text(request_id, request));
            }
            ChatEvent::AcpNotification { payload } => {
                self.apply_acp_notification(payload);
            }
            ChatEvent::Error { error } => {
                self.last_error = Some(error.clone());
                self.work_status = None;
                self.push_notice(format!("Error: {error}"));
            }
            ChatEvent::PromptDone { .. } => {
                self.work_status = None;
            }
            ChatEvent::TurnStatus { active } => {
                self.work_status = None;
                if *active {
                    self.last_action = None;
                }
            }
            ChatEvent::SessionMode { .. }
            | ChatEvent::CommandMenu { .. }
            | ChatEvent::MultiAgentTurn { .. }
            | ChatEvent::SubagentStatus { .. }
            | ChatEvent::SubagentAcpNotification { .. } => {}
        }
        self.chat_state.apply_event(event);
    }

    fn apply_acp_notification(&mut self, payload: &Value) {
        let Some(update) = payload.get("update") else {
            return;
        };
        match update.get("sessionUpdate").and_then(Value::as_str) {
            Some("agent_message_chunk") => {
                if let Some(text) = content_text(update.get("content")) {
                    self.append_response_text(text);
                }
            }
            Some("user_message_chunk") => {
                if let Some(text) = content_text(update.get("content")) {
                    self.append_request_echo(text);
                }
            }
            Some("agent_thought_chunk") => {
                if let Some(text) = content_text(update.get("content")) {
                    self.work_status = Some(format!("Thought: {}", one_line(text)));
                }
            }
            Some("tool_call") | Some("tool_call_update") => {
                self.work_status = Some(tool_activity_text(update));
            }
            Some("plan") => {
                self.push_notice("Plan updated.");
            }
            _ => {}
        }
    }

    fn append_response_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(message) = self.chat_messages.last_mut() {
            if message.role == ChatRole::Response {
                message.text.push_str(text);
                return;
            }
        }
        self.chat_messages.push(ChatMessage {
            role: ChatRole::Response,
            text: text.to_string(),
        });
    }

    fn append_request_echo(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self
            .chat_messages
            .last()
            .is_some_and(|message| message.role == ChatRole::Request && message.text == text)
        {
            return;
        }
        self.chat_messages.push(ChatMessage {
            role: ChatRole::Request,
            text: text.to_string(),
        });
    }
}
