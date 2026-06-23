use serde_json::Value;
use tokio::sync::mpsc;
use va_client::events::{ChatClientMessage, ChatEvent, ChatSessionAction};

use crate::app::TuiApp;
use crate::chat::{
    content_text, one_line, permission_prompt_text, resolve_permission_option,
    resolve_session_mode_value, session_mode_options_text, tool_activity_text, ChatMessage,
    ChatRole, SessionModeSource,
};
use crate::chat_socket::ChatSocketEvent;
use crate::transport::HttpTransport;

impl TuiApp {
    pub(crate) fn insert_chat_text(&mut self, text: &str) {
        self.chat_input.push_str(&normalize_input_text(text));
    }

    pub(crate) fn insert_chat_newline(&mut self) {
        self.chat_input.push('\n');
    }

    pub(crate) fn delete_chat_char(&mut self) {
        self.chat_input.pop();
    }

    pub(crate) fn clear_chat_input(&mut self) {
        self.chat_input.clear();
    }

    pub(crate) fn delete_chat_word(&mut self) {
        let trimmed_len = self.chat_input.trim_end_matches(char::is_whitespace).len();
        self.chat_input.truncate(trimmed_len);
        let word_start = self
            .chat_input
            .char_indices()
            .rev()
            .find(|(_, ch)| ch.is_whitespace())
            .map(|(index, ch)| index + ch.len_utf8())
            .unwrap_or(0);
        self.chat_input.truncate(word_start);
    }

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
        if input.starts_with('/') && self.run_slash_command(&input, transport, chat_tx).await {
            return;
        }

        self.submit_user_message(input, chat_tx);
    }

    fn submit_user_message(
        &mut self,
        input: String,
        chat_tx: &mpsc::UnboundedSender<ChatClientMessage>,
    ) {
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
    ) -> bool {
        let (name, args) = split_slash_command(command);
        match name {
            "/status" => {
                self.open_status(transport).await;
                true
            }
            "/agent" => {
                self.open_agent_picker(transport).await;
                true
            }
            "/help" => {
                self.push_help_message();
                true
            }
            "/clear" => {
                self.chat_messages.clear();
                self.follow_chat_tail();
                true
            }
            "/new" => {
                self.prepare_new_chat_session();
                true
            }
            "/resume" | "/session" => {
                if let Some(session_id) = args {
                    self.resume_chat_session(session_id, chat_tx);
                } else {
                    self.push_notice("Usage: /resume <session-id>");
                }
                true
            }
            "/mode" => {
                if let Some(mode_id) = args {
                    self.set_chat_mode(mode_id, chat_tx);
                } else {
                    self.push_notice(
                        session_mode_options_text(self.chat_state.session_mode.as_ref())
                            .unwrap_or_else(|| {
                                "Usage: /mode default|plan|accept|bypass|dontask".into()
                            }),
                    );
                }
                true
            }
            "/back" => {
                self.go_back();
                true
            }
            "/stop" => {
                self.send_chat_command(ChatClientMessage::stop(), chat_tx);
                true
            }
            "/allow" => {
                let selector = args;
                if let Some(permission) = self.chat_state.pending_permission.clone() {
                    if let Some(option_id) =
                        resolve_permission_option(&permission.request, selector)
                    {
                        if self.send_chat_command(
                            ChatClientMessage::permission_selected(
                                permission.request_id.clone(),
                                option_id,
                            ),
                            chat_tx,
                        ) {
                            self.clear_pending_permission_after_response();
                        }
                    } else {
                        self.push_notice(
                            "Unknown permission option. Use /allow [number|option-id] or /deny.",
                        );
                    }
                } else {
                    self.push_notice("No pending permission request.");
                }
                true
            }
            "/deny" | "/cancel" => {
                if let Some(request_id) = self.chat_state.pending_permission_request_id.clone() {
                    if self.send_chat_command(
                        ChatClientMessage::permission_cancelled(request_id),
                        chat_tx,
                    ) {
                        self.clear_pending_permission_after_response();
                    }
                } else {
                    self.push_notice("No pending permission request.");
                }
                true
            }
            _ => false,
        }
    }

    fn push_help_message(&mut self) {
        self.chat_messages.push(ChatMessage {
            role: ChatRole::Notice,
            text: "/new next message starts a new session  /resume session-id resume a session  /mode list or set permission mode  /status runtime status  /agent agent context  /stop stop turn  /allow [number|option-id]  /deny  /clear clear chat  Shift+Enter newline  Ctrl+U clear input  Ctrl+W delete word".into(),
        });
    }

    fn push_notice(&mut self, text: impl Into<String>) {
        self.chat_messages.push(ChatMessage {
            role: ChatRole::Notice,
            text: text.into(),
        });
    }

    fn clear_pending_permission_after_response(&mut self) {
        self.chat_state.pending_permission_request_id = None;
        self.chat_state.pending_permission = None;
        self.last_action = Some("permission response sent".into());
    }

    pub(crate) fn send_chat_message(
        &mut self,
        text: String,
        chat_tx: &mpsc::UnboundedSender<ChatClientMessage>,
    ) {
        let force_new_session = self.force_new_session;
        let message = ChatClientMessage::Message {
            text,
            message_id: None,
            agent: self.effective_agent().map(str::to_string),
            profile_id: self.effective_profile().map(str::to_string),
            session_action: if force_new_session {
                Some(ChatSessionAction::New)
            } else {
                self.effective_session().map(|_| ChatSessionAction::Resume)
            },
            session_id: if force_new_session {
                None
            } else {
                self.effective_session().map(str::to_string)
            },
            session_workspace: self.effective_workspace().map(str::to_string),
            permission_mode: None,
            attachments: Vec::new(),
        };
        if self.send_chat_command(message, chat_tx) && force_new_session {
            self.force_new_session = false;
        }
    }

    fn send_chat_command(
        &mut self,
        message: ChatClientMessage,
        chat_tx: &mpsc::UnboundedSender<ChatClientMessage>,
    ) -> bool {
        if chat_tx.send(message).is_err() {
            self.last_error = Some("chat websocket task is not running".into());
            return false;
        }
        true
    }

    fn prepare_new_chat_session(&mut self) {
        if self.chat_state.turn_active {
            self.push_notice("Stop or wait for the current turn before starting a new session.");
            return;
        }
        self.force_new_session = true;
        self.selected_session = None;
        self.chat_state.session_id = None;
        self.work_status = None;
        self.last_error = None;
        self.last_action = Some("next message starts a new session".into());
        self.push_notice("Next message will start a new session.");
        self.follow_chat_tail();
    }

    fn set_chat_mode(&mut self, mode_id: &str, chat_tx: &mpsc::UnboundedSender<ChatClientMessage>) {
        if let Some(state) =
            crate::chat::parse_session_mode_state(self.chat_state.session_mode.as_ref())
        {
            let Some(mode_value) =
                resolve_session_mode_value(self.chat_state.session_mode.as_ref(), mode_id)
            else {
                self.push_notice(
                    session_mode_options_text(self.chat_state.session_mode.as_ref())
                        .unwrap_or_else(|| "Unknown mode.".into()),
                );
                return;
            };
            let message = match state.source {
                SessionModeSource::ConfigOption => {
                    let Some(config_id) = state.config_id else {
                        self.push_notice("Session mode config is missing a config id.");
                        return;
                    };
                    ChatClientMessage::set_config_option(config_id, mode_value.clone())
                }
                SessionModeSource::SessionMode => ChatClientMessage::set_mode(mode_value.clone()),
            };
            if self.send_chat_command(message, chat_tx) {
                self.last_action = Some(format!("requested mode {mode_value}"));
            }
            return;
        }

        let Some(mode_value) = canonical_chat_mode(mode_id) else {
            self.push_notice("Unknown mode. Valid: default, plan, accept, bypass, dontask.");
            return;
        };
        if self.send_chat_command(ChatClientMessage::set_mode(mode_value), chat_tx) {
            self.last_action = Some(format!("requested mode {mode_value}"));
        }
    }

    fn resume_chat_session(
        &mut self,
        session_id: &str,
        chat_tx: &mpsc::UnboundedSender<ChatClientMessage>,
    ) {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            self.push_notice("Usage: /resume <session-id>");
            return;
        }
        let message = ChatClientMessage::resume_session_with_options(
            session_id,
            self.effective_agent().map(str::to_string),
            self.effective_profile().map(str::to_string),
            self.effective_workspace().map(str::to_string),
        );
        if self.send_chat_command(message, chat_tx) {
            self.selected_session = Some(session_id.to_string());
            self.force_new_session = false;
            self.last_action = Some(format!("resuming session {}", short_id(session_id)));
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
            ChatEvent::SessionReady { session_id } => {
                self.selected_session = Some(session_id.clone());
                self.force_new_session = false;
            }
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

fn short_id(value: &str) -> String {
    value.chars().take(12).collect()
}

fn canonical_chat_mode(mode_id: &str) -> Option<&'static str> {
    match mode_id
        .trim()
        .to_ascii_lowercase()
        .replace(['_', '-'], "")
        .as_str()
    {
        "default" => Some("default"),
        "plan" => Some("plan"),
        "accept" | "acceptedits" => Some("acceptEdits"),
        "bypass" | "bypasspermissions" => Some("bypassPermissions"),
        "dontask" => Some("dontAsk"),
        _ => None,
    }
}

fn normalize_input_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn split_slash_command(command: &str) -> (&str, Option<&str>) {
    let command = command.trim();
    match command.find(char::is_whitespace) {
        Some(index) => {
            let name = &command[..index];
            let args = command[index..].trim();
            (name, (!args.is_empty()).then_some(args))
        }
        None => (command, None),
    }
}
