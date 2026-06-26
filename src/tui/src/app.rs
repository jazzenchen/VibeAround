use std::time::{Duration, Instant};

use va_client::endpoint::ServerEndpoint;
use va_client::state::ChatState;

use crate::chat::{ChatMessage, ChatRole};
use crate::data::{
    fetch_agent_picker, fetch_launcher_preferences, fetch_snapshot, AgentPickerSnapshot,
    DashboardSnapshot,
};
use crate::popup::Popup;
use crate::runtime_socket::RuntimeStream;
use crate::transport::HttpTransport;

mod chat;
mod popup;
mod runtime;

const EXIT_CONFIRM_WINDOW: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub(crate) struct TuiApp {
    pub(crate) view: AppView,
    pub(crate) chat_state: ChatState,
    pub(crate) chat_connected: bool,
    pub(crate) snapshot: DashboardSnapshot,
    pub(crate) agent_picker: AgentPickerSnapshot,
    pub(crate) chat_messages: Vec<ChatMessage>,
    pub(crate) chat_input: String,
    pub(crate) chat_cursor: usize,
    pub(crate) chat_scroll: usize,
    /// Previously submitted inputs (messages and commands), oldest first.
    input_history: Vec<String>,
    /// Position within [`Self::input_history`] while recalling, or `None`
    /// when editing the live draft.
    history_cursor: Option<usize>,
    /// The live draft, parked while the user browses history.
    history_draft: String,
    /// Highlighted entry in the slash-command autocomplete popup.
    pub(crate) slash_selection: usize,
    /// The last submitted input and when, used to drop an immediate duplicate
    /// (e.g. an IME that fires the commit Enter twice).
    last_submit: Option<(Instant, String)>,
    /// Active bottom-up command popup (`/status`, `/agent`), if any.
    pub(crate) popup: Option<Popup>,
    pub(crate) selected_agent: Option<String>,
    pub(crate) selected_profile: Option<String>,
    pub(crate) selected_workspace: Option<String>,
    pub(crate) selected_session: Option<String>,
    pub(crate) force_new_session: bool,
    pub(crate) work_status: Option<String>,
    /// When the current agent turn started, used to drive the live working
    /// indicator (spinner + elapsed) in the transcript.
    pub(crate) turn_started_at: Option<Instant>,
    pub(crate) last_error: Option<String>,
    last_error_scope: Option<ErrorScope>,
    pub(crate) last_action: Option<String>,
    pub(crate) last_refresh: Option<Instant>,
    exit_confirmation_started: Option<Instant>,
}

impl TuiApp {
    pub(crate) fn new(_endpoint: &ServerEndpoint) -> Self {
        Self {
            view: AppView::Chat,
            chat_state: ChatState::new(),
            chat_connected: false,
            snapshot: DashboardSnapshot::default(),
            agent_picker: AgentPickerSnapshot::default(),
            // Start clean — the welcome screen's tip and the footer cover
            // command discovery, so no seed notice in the transcript.
            chat_messages: Vec::new(),
            chat_input: String::new(),
            chat_cursor: 0,
            chat_scroll: 0,
            input_history: Vec::new(),
            history_cursor: None,
            history_draft: String::new(),
            slash_selection: 0,
            last_submit: None,
            popup: None,
            selected_agent: None,
            selected_profile: None,
            selected_workspace: None,
            selected_session: None,
            force_new_session: false,
            work_status: None,
            turn_started_at: None,
            last_error: None,
            last_error_scope: None,
            last_action: None,
            last_refresh: None,
            exit_confirmation_started: None,
        }
    }

    pub(crate) async fn refresh_status(&mut self, transport: &HttpTransport) {
        match fetch_snapshot(transport).await {
            Ok(snapshot) => {
                self.snapshot = snapshot;
                self.clear_error(ErrorScope::Status);
                self.last_refresh = Some(Instant::now());
            }
            Err(error) => {
                self.set_error(ErrorScope::Status, error.to_string());
                self.last_refresh = Some(Instant::now());
            }
        }
    }

    /// Seed the chat context from the launcher's current selection so the
    /// header shows the real agent/profile/workspace at startup, not `global`.
    pub(crate) async fn sync_launcher_context(&mut self, transport: &HttpTransport) {
        if let Ok(preferences) = fetch_launcher_preferences(transport).await {
            self.agent_picker.preferences = Some(preferences);
            self.clear_error(ErrorScope::Agent);
        }
    }

    pub(crate) async fn refresh_agent_picker(&mut self, transport: &HttpTransport) {
        match fetch_agent_picker(transport).await {
            Ok(snapshot) => {
                self.agent_picker = snapshot;
                self.clear_error(ErrorScope::Agent);
                self.last_refresh = Some(Instant::now());
            }
            Err(error) => {
                self.set_error(ErrorScope::Agent, error.to_string());
                self.last_refresh = Some(Instant::now());
            }
        }
    }

    /// Esc with no popup open clears the input draft.
    pub(crate) fn go_back(&mut self) {
        self.clear_chat_input();
    }

    pub(crate) fn scroll_chat_up(&mut self, lines: usize) {
        self.chat_scroll = self.chat_scroll.saturating_add(lines);
    }

    pub(crate) fn scroll_chat_down(&mut self, lines: usize) {
        self.chat_scroll = self.chat_scroll.saturating_sub(lines);
    }

    pub(crate) fn follow_chat_tail(&mut self) {
        self.chat_scroll = 0;
    }

    /// The chat has not started a real exchange yet (only passive notices).
    /// Drives the centered welcome/splash screen.
    pub(crate) fn is_welcome(&self) -> bool {
        self.view == AppView::Chat
            && !self.chat_messages.iter().any(|message| {
                matches!(
                    message.role,
                    ChatRole::Request | ChatRole::Response | ChatRole::Work
                )
            })
    }

    pub(crate) fn effective_agent(&self) -> Option<&str> {
        self.selected_agent.as_deref().or_else(|| {
            self.agent_picker
                .preferences
                .as_ref()
                .map(|preferences| preferences.selected_agent.as_str())
        })
    }

    pub(crate) fn effective_profile(&self) -> Option<&str> {
        self.selected_profile.as_deref().or_else(|| {
            let preferences = self.agent_picker.preferences.as_ref()?;
            let agent_id = self.effective_agent()?;
            preferences
                .agent_preferences
                .get(agent_id)
                .and_then(|preference| preference.profile_id.as_deref())
                .or(preferences.default_profile_id.as_deref())
        })
    }

    pub(crate) fn effective_workspace(&self) -> Option<&str> {
        self.selected_workspace.as_deref().or_else(|| {
            let preferences = self.agent_picker.preferences.as_ref()?;
            let agent_id = self.effective_agent()?;
            preferences
                .agent_preferences
                .get(agent_id)
                .and_then(|preference| preference.workspace.as_deref())
        })
    }

    pub(crate) fn effective_session(&self) -> Option<&str> {
        self.selected_session.as_deref()
    }

    pub(crate) fn confirm_exit_request(&mut self) -> bool {
        self.confirm_exit_request_at(Instant::now())
    }

    fn confirm_exit_request_at(&mut self, now: Instant) -> bool {
        if self.exit_confirmation_active_at(now) {
            self.exit_confirmation_started = None;
            return true;
        }

        self.exit_confirmation_started = Some(now);
        self.clear_error(ErrorScope::Any);
        self.last_action = None;
        false
    }

    pub(crate) fn set_error(&mut self, scope: ErrorScope, error: impl Into<String>) {
        self.last_error = Some(error.into());
        self.last_error_scope = Some(scope);
    }

    pub(crate) fn clear_error(&mut self, scope: ErrorScope) {
        if scope == ErrorScope::Any
            || self.last_error_scope.is_none()
            || self.last_error_scope == Some(scope)
        {
            self.last_error = None;
            self.last_error_scope = None;
        }
    }

    pub(crate) fn error_is(&self, scope: ErrorScope, error: &str) -> bool {
        self.last_error_scope == Some(scope) && self.last_error.as_deref() == Some(error)
    }

    pub(crate) fn clear_expired_exit_confirmation(&mut self) {
        self.clear_expired_exit_confirmation_at(Instant::now());
    }

    fn clear_expired_exit_confirmation_at(&mut self, now: Instant) {
        if self.exit_confirmation_started.is_some() && !self.exit_confirmation_active_at(now) {
            self.exit_confirmation_started = None;
        }
    }

    fn exit_confirmation_active_at(&self, now: Instant) -> bool {
        self.exit_confirmation_started
            .and_then(|started| now.checked_duration_since(started))
            .is_some_and(|elapsed| elapsed <= EXIT_CONFIRM_WINDOW)
    }

    pub(crate) fn exit_confirmation_pending(&self) -> bool {
        self.exit_confirmation_started.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AppView {
    Chat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ErrorScope {
    Any,
    Agent,
    Chat,
    Runtime(RuntimeStream),
    Status,
}

#[cfg(test)]
mod tests;
