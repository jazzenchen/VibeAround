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
        let text = normalize_input_text(text);
        self.clamp_chat_cursor();
        self.chat_input.insert_str(self.chat_cursor, &text);
        self.chat_cursor += text.len();
    }

    pub(crate) fn insert_chat_newline(&mut self) {
        self.insert_chat_text("\n");
    }

    pub(crate) fn delete_chat_char(&mut self) {
        self.clamp_chat_cursor();
        if self.chat_cursor == 0 {
            return;
        }
        let previous = previous_boundary(&self.chat_input, self.chat_cursor);
        self.chat_input
            .replace_range(previous..self.chat_cursor, "");
        self.chat_cursor = previous;
    }

    pub(crate) fn clear_chat_input(&mut self) {
        self.chat_input.clear();
        self.chat_cursor = 0;
    }

    pub(crate) fn delete_chat_word(&mut self) {
        self.clamp_chat_cursor();
        let before_cursor = &self.chat_input[..self.chat_cursor];
        let trimmed_len = before_cursor.trim_end_matches(char::is_whitespace).len();
        let word_start = self.chat_input[..trimmed_len]
            .char_indices()
            .rev()
            .find(|(_, ch)| ch.is_whitespace())
            .map(|(index, ch)| index + ch.len_utf8())
            .unwrap_or(0);
        let delete_end = if self.chat_input[..word_start]
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace)
            && self.chat_input[self.chat_cursor..]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
        {
            next_boundary(&self.chat_input, self.chat_cursor)
        } else {
            self.chat_cursor
        };
        self.chat_input.replace_range(word_start..delete_end, "");
        self.chat_cursor = word_start;
    }

    pub(crate) fn move_chat_cursor_left(&mut self) {
        self.clamp_chat_cursor();
        self.chat_cursor = previous_boundary(&self.chat_input, self.chat_cursor);
    }

    pub(crate) fn move_chat_cursor_right(&mut self) {
        self.clamp_chat_cursor();
        if self.chat_cursor >= self.chat_input.len() {
            return;
        }
        self.chat_cursor = next_boundary(&self.chat_input, self.chat_cursor);
    }

    pub(crate) fn move_chat_cursor_start(&mut self) {
        self.chat_cursor = 0;
    }

    pub(crate) fn move_chat_cursor_end(&mut self) {
        self.chat_cursor = self.chat_input.len();
    }

    fn clamp_chat_cursor(&mut self) {
        self.chat_cursor = clamp_boundary(&self.chat_input, self.chat_cursor);
    }

    #[cfg(test)]
    pub(crate) fn sync_chat_cursor_to_input_end(&mut self) {
        self.chat_cursor = self.chat_input.len();
    }

    #[cfg(test)]
    pub(crate) fn set_chat_input_for_test(&mut self, input: impl Into<String>) {
        self.chat_input = input.into();
        self.sync_chat_cursor_to_input_end();
    }

    #[cfg(test)]
    pub(crate) fn set_chat_cursor_for_test(&mut self, cursor: usize) {
        self.chat_cursor = clamp_boundary(&self.chat_input, cursor);
    }

    pub(crate) async fn submit_chat_input(
        &mut self,
        transport: &HttpTransport,
        chat_tx: &mpsc::UnboundedSender<ChatClientMessage>,
    ) {
        let input = self.chat_input.trim().to_string();
        self.chat_input.clear();
        self.chat_cursor = 0;
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
                                option_id.clone(),
                            ),
                            chat_tx,
                        ) {
                            self.clear_pending_permission_after_response(format!(
                                "permission selected: {option_id}"
                            ));
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
                        self.clear_pending_permission_after_response("permission denied");
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
            text: "Commands\n/status runtime status\n/agent agent, profile, workspace, session\n/new next message starts a new session\n/resume <session-id> resume a session\n/mode list or set permission mode\n/stop stop current turn\n/allow [number|option-id] answer permission\n/deny reject permission\n/clear clear chat\nShift+Enter newline, Ctrl+U clear input, Ctrl+W delete word".into(),
        });
    }

    fn push_notice(&mut self, text: impl Into<String>) {
        self.chat_messages.push(ChatMessage {
            role: ChatRole::Notice,
            text: text.into(),
        });
    }

    fn clear_pending_permission_after_response(&mut self, action: impl Into<String>) {
        self.chat_state.pending_permission_request_id = None;
        self.chat_state.pending_permission = None;
        self.last_action = Some(action.into());
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
                self.push_response_text(text);
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
                    self.append_stream_response_text(text);
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

    fn append_stream_response_text(&mut self, text: &str) {
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

    fn push_response_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
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

fn clamp_boundary(input: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(input.len());
    while cursor > 0 && !input.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

fn previous_boundary(input: &str, cursor: usize) -> usize {
    let cursor = clamp_boundary(input, cursor);
    if cursor == 0 {
        return 0;
    }
    input[..cursor]
        .char_indices()
        .next_back()
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn next_boundary(input: &str, cursor: usize) -> usize {
    let cursor = clamp_boundary(input, cursor);
    if cursor >= input.len() {
        return input.len();
    }
    let mut indices = input[cursor..].char_indices();
    let _ = indices.next();
    indices
        .next()
        .map(|(index, _)| cursor + index)
        .unwrap_or(input.len())
}
