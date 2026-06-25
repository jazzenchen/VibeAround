use serde_json::Value;
use tokio::sync::mpsc;
use va_client::events::{ChatClientMessage, ChatEvent, ChatSessionAction};

use crate::app::{ErrorScope, TuiApp};
use crate::chat::{
    content_text, one_line, permission_prompt_text, resolve_permission_option,
    resolve_session_mode_value, session_mode_options_text, slash_command_matches, tool_activity_text,
    ChatMessage, ChatRole, SessionModeSource, SlashCommand,
};
use crate::chat_socket::ChatSocketEvent;
use crate::transport::HttpTransport;

impl TuiApp {
    pub(crate) fn insert_chat_text(&mut self, text: &str) {
        self.note_input_edited();
        let text = normalize_input_text(text);
        self.clamp_chat_cursor();
        self.chat_input.insert_str(self.chat_cursor, &text);
        self.chat_cursor += text.len();
    }

    /// Step back to an earlier submitted input. The first step parks the live
    /// draft so [`Self::history_next`] can restore it.
    pub(crate) fn history_prev(&mut self) {
        if self.input_history.is_empty() {
            return;
        }
        let index = match self.history_cursor {
            None => {
                self.history_draft = self.chat_input.clone();
                self.input_history.len() - 1
            }
            Some(0) => return,
            Some(current) => current - 1,
        };
        self.set_input_from_history(index);
    }

    /// Step forward toward newer inputs; past the newest entry, restore the
    /// parked draft.
    pub(crate) fn history_next(&mut self) {
        match self.history_cursor {
            None => {}
            Some(current) if current + 1 < self.input_history.len() => {
                self.set_input_from_history(current + 1);
            }
            Some(_) => {
                self.history_cursor = None;
                self.chat_input = std::mem::take(&mut self.history_draft);
                self.chat_cursor = self.chat_input.len();
            }
        }
    }

    fn set_input_from_history(&mut self, index: usize) {
        self.history_cursor = Some(index);
        self.chat_input = self.input_history[index].clone();
        self.chat_cursor = self.chat_input.len();
    }

    /// Record a submitted input and leave history-browsing mode.
    fn record_input_history(&mut self, entry: &str) {
        const MAX_HISTORY: usize = 200;
        self.history_cursor = None;
        self.history_draft.clear();
        let entry = entry.trim();
        if entry.is_empty() {
            return;
        }
        if self.input_history.last().map(String::as_str) == Some(entry) {
            return;
        }
        self.input_history.push(entry.to_string());
        if self.input_history.len() > MAX_HISTORY {
            let excess = self.input_history.len() - MAX_HISTORY;
            self.input_history.drain(0..excess);
        }
    }

    /// Editing the input exits history-browsing and re-anchors the
    /// autocomplete popup to its first entry.
    fn note_input_edited(&mut self) {
        self.history_cursor = None;
        self.slash_selection = 0;
    }

    /// The slash commands matching the input being typed, or `None` when the
    /// autocomplete popup should be hidden.
    pub(crate) fn slash_matches(&self) -> Option<Vec<&'static SlashCommand>> {
        slash_command_matches(&self.chat_input)
    }

    pub(crate) fn slash_popup_open(&self) -> bool {
        self.slash_matches().is_some()
    }

    /// Selected entry, clamped to the current match list.
    pub(crate) fn slash_selected(&self) -> Option<&'static SlashCommand> {
        let matches = self.slash_matches()?;
        matches
            .get(self.slash_selection.min(matches.len().saturating_sub(1)))
            .copied()
    }

    pub(crate) fn slash_select_prev(&mut self) {
        if let Some(matches) = self.slash_matches() {
            let last = matches.len().saturating_sub(1);
            let current = self.slash_selection.min(last);
            self.slash_selection = if current == 0 { last } else { current - 1 };
        }
    }

    pub(crate) fn slash_select_next(&mut self) {
        if let Some(matches) = self.slash_matches() {
            let last = matches.len().saturating_sub(1);
            let current = self.slash_selection.min(last);
            self.slash_selection = if current == last { 0 } else { current + 1 };
        }
    }

    /// Replace the input with the highlighted command. With `trailing_space`
    /// the cursor is parked after a space, ready for arguments (Tab); without
    /// it the bare command is left for immediate submission (Enter).
    pub(crate) fn accept_slash_selection(&mut self, trailing_space: bool) {
        if let Some(command) = self.slash_selected() {
            self.chat_input = if trailing_space {
                format!("{} ", command.name)
            } else {
                command.name.to_string()
            };
            self.chat_cursor = self.chat_input.len();
            self.history_cursor = None;
            self.slash_selection = 0;
        }
    }

    pub(crate) fn insert_chat_newline(&mut self) {
        self.insert_chat_text("\n");
    }

    pub(crate) fn delete_chat_char(&mut self) {
        self.note_input_edited();
        self.clamp_chat_cursor();
        if self.chat_cursor == 0 {
            return;
        }
        let previous = previous_boundary(&self.chat_input, self.chat_cursor);
        self.chat_input
            .replace_range(previous..self.chat_cursor, "");
        self.chat_cursor = previous;
    }

    pub(crate) fn delete_chat_forward_char(&mut self) {
        self.note_input_edited();
        self.clamp_chat_cursor();
        if self.chat_cursor >= self.chat_input.len() {
            return;
        }
        let next = next_boundary(&self.chat_input, self.chat_cursor);
        self.chat_input.replace_range(self.chat_cursor..next, "");
    }

    pub(crate) fn delete_chat_to_end(&mut self) {
        self.note_input_edited();
        self.clamp_chat_cursor();
        self.chat_input.truncate(self.chat_cursor);
    }

    pub(crate) fn clear_chat_input(&mut self) {
        self.note_input_edited();
        self.chat_input.clear();
        self.chat_cursor = 0;
    }

    pub(crate) fn delete_chat_word(&mut self) {
        self.note_input_edited();
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

    pub(crate) fn move_chat_cursor_word_left(&mut self) {
        self.clamp_chat_cursor();
        let before_cursor = &self.chat_input[..self.chat_cursor];
        let trimmed_len = before_cursor.trim_end_matches(char::is_whitespace).len();
        self.chat_cursor = self.chat_input[..trimmed_len]
            .char_indices()
            .rev()
            .find(|(_, ch)| ch.is_whitespace())
            .map(|(index, ch)| index + ch.len_utf8())
            .unwrap_or(0);
    }

    pub(crate) fn move_chat_cursor_word_right(&mut self) {
        self.clamp_chat_cursor();
        if self.chat_cursor >= self.chat_input.len() {
            return;
        }

        let mut saw_word = false;
        for (relative_index, ch) in self.chat_input[self.chat_cursor..].char_indices() {
            if ch.is_whitespace() {
                if saw_word {
                    self.chat_cursor += relative_index;
                    return;
                }
            } else {
                saw_word = true;
            }
        }
        self.chat_cursor = self.chat_input.len();
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
        let input = self.chat_input.clone();
        self.chat_input.clear();
        self.chat_cursor = 0;
        let command = input.trim();
        if command.is_empty() {
            return;
        }
        self.record_input_history(command);
        if command.starts_with('/') && self.run_slash_command(command, transport, chat_tx).await {
            return;
        }

        self.submit_user_message(input, chat_tx);
    }

    fn submit_user_message(
        &mut self,
        input: String,
        chat_tx: &mpsc::UnboundedSender<ChatClientMessage>,
    ) {
        let input_cursor = input.len();
        self.work_status = None;
        if self.send_chat_message(input.clone(), chat_tx) {
            self.chat_messages.push(ChatMessage {
                role: ChatRole::Request,
                text: input,
            });
            self.follow_chat_tail();
        } else {
            self.chat_input = input;
            self.chat_cursor = input_cursor;
        }
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
                    self.push_local_notice("Usage: /resume <session-id>");
                }
                true
            }
            "/mode" => {
                if let Some(mode_id) = args {
                    self.set_chat_mode(mode_id, chat_tx);
                } else {
                    self.push_local_notice(
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
                        self.push_local_notice(
                            "Unknown permission option. Use /allow [number|option-id] or /deny.",
                        );
                    }
                } else {
                    self.push_local_notice("No pending permission request.");
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
                    self.push_local_notice("No pending permission request.");
                }
                true
            }
            _ => false,
        }
    }

    fn push_help_message(&mut self) {
        self.chat_messages.push(ChatMessage {
            role: ChatRole::Notice,
            text: "Commands\n/status runtime status\n/agent agent, profile, workspace, session\n/new next message starts a new session\n/resume <session-id> resume a session\n/mode list or set permission mode\n/stop stop current turn\n/allow [number|option-id] answer permission\n/deny reject permission\n/clear clear chat\nShift+Enter newline, Left/Right edit, Alt+Left/Right word, Ctrl+A/E start/end, Ctrl+U clear, Ctrl+W delete word, Ctrl+K delete tail".into(),
        });
        self.follow_chat_tail();
    }

    fn push_notice(&mut self, text: impl Into<String>) {
        self.chat_messages.push(ChatMessage {
            role: ChatRole::Notice,
            text: text.into(),
        });
    }

    fn push_local_notice(&mut self, text: impl Into<String>) {
        self.push_notice(text);
        self.follow_chat_tail();
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
    ) -> bool {
        let force_new_session = self.force_new_session;
        let message = ChatClientMessage::Message {
            text,
            message_id: Some(uuid::Uuid::new_v4().to_string()),
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
        let sent = self.send_chat_command(message, chat_tx);
        if sent && force_new_session {
            self.force_new_session = false;
        }
        sent
    }

    fn send_chat_command(
        &mut self,
        message: ChatClientMessage,
        chat_tx: &mpsc::UnboundedSender<ChatClientMessage>,
    ) -> bool {
        if chat_tx.send(message).is_err() {
            self.set_error(ErrorScope::Chat, "chat websocket task is not running");
            return false;
        }
        true
    }

    fn prepare_new_chat_session(&mut self) {
        if self.chat_state.turn_active {
            self.push_local_notice(
                "Stop or wait for the current turn before starting a new session.",
            );
            return;
        }
        self.force_new_session = true;
        self.selected_session = None;
        self.chat_state.session_id = None;
        self.work_status = None;
        self.clear_error(ErrorScope::Chat);
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
                self.push_local_notice(
                    session_mode_options_text(self.chat_state.session_mode.as_ref())
                        .unwrap_or_else(|| "Unknown mode.".into()),
                );
                return;
            };
            let message = match state.source {
                SessionModeSource::ConfigOption => {
                    let Some(config_id) = state.config_id else {
                        self.push_local_notice("Session mode config is missing a config id.");
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
            self.push_local_notice("Unknown mode. Valid: default, plan, accept, bypass, dontask.");
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
            self.push_local_notice("Usage: /resume <session-id>");
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
                self.clear_error(ErrorScope::Chat);
            }
            ChatSocketEvent::Closed => {
                let duplicate_closed = self.last_notice_is("Chat websocket closed.");
                self.chat_connected = false;
                if !duplicate_closed {
                    self.push_notice("Chat websocket closed.");
                }
            }
            ChatSocketEvent::Error(error) => {
                let duplicate_error = self.error_is(ErrorScope::Chat, &error);
                self.chat_connected = false;
                self.set_error(ErrorScope::Chat, error.clone());
                if !duplicate_error {
                    self.push_notice(format!("Chat websocket error: {error}"));
                }
            }
            ChatSocketEvent::Event(event) => self.apply_chat_event(event),
        }
    }

    fn last_notice_is(&self, text: &str) -> bool {
        self.chat_messages
            .last()
            .is_some_and(|message| message.role == ChatRole::Notice && message.text == text)
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
                self.set_error(ErrorScope::Chat, error.clone());
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
