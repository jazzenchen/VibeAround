use std::time::{Duration, Instant};

use va_client::endpoint::ServerEndpoint;
use va_client::state::ChatState;

use crate::chat::{ChatMessage, ChatRole};
use crate::data::{fetch_agent_picker, fetch_snapshot, AgentPickerSnapshot, DashboardSnapshot};
use crate::detail::DetailContent;
use crate::selection::{AgentPanel, AgentSelection, StatusSelection};
use crate::transport::HttpTransport;

mod chat;

const EXIT_CONFIRM_WINDOW: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub(crate) struct TuiApp {
    pub(crate) endpoint: String,
    pub(crate) view: AppView,
    pub(crate) chat_state: ChatState,
    pub(crate) chat_connected: bool,
    pub(crate) snapshot: DashboardSnapshot,
    pub(crate) agent_picker: AgentPickerSnapshot,
    pub(crate) status_selection: StatusSelection,
    pub(crate) agent_selection: AgentSelection,
    pub(crate) chat_messages: Vec<ChatMessage>,
    pub(crate) chat_input: String,
    pub(crate) chat_scroll: usize,
    pub(crate) selected_agent: Option<String>,
    pub(crate) selected_profile: Option<String>,
    pub(crate) selected_workspace: Option<String>,
    pub(crate) selected_session: Option<String>,
    pub(crate) detail: Option<DetailContent>,
    pub(crate) work_status: Option<String>,
    pub(crate) last_error: Option<String>,
    pub(crate) last_action: Option<String>,
    pub(crate) last_refresh: Option<Instant>,
    exit_confirmation_started: Option<Instant>,
}

impl TuiApp {
    pub(crate) fn new(endpoint: &ServerEndpoint) -> Self {
        Self {
            endpoint: endpoint.base_url().to_string(),
            view: AppView::Chat,
            chat_state: ChatState::new(),
            chat_connected: false,
            snapshot: DashboardSnapshot::default(),
            agent_picker: AgentPickerSnapshot::default(),
            status_selection: StatusSelection::default(),
            agent_selection: AgentSelection::default(),
            chat_messages: vec![ChatMessage {
                role: ChatRole::Notice,
                text: "Type /status for runtime status, /agent for agent settings, /help for commands.".into(),
            }],
            chat_input: String::new(),
            chat_scroll: 0,
            selected_agent: None,
            selected_profile: None,
            selected_workspace: None,
            selected_session: None,
            detail: None,
            work_status: None,
            last_error: None,
            last_action: None,
            last_refresh: None,
            exit_confirmation_started: None,
        }
    }

    pub(crate) async fn refresh_status(&mut self, transport: &HttpTransport) {
        match fetch_snapshot(transport).await {
            Ok(snapshot) => {
                self.snapshot = snapshot;
                self.status_selection.clamp(&self.snapshot);
                self.last_error = None;
                self.last_refresh = Some(Instant::now());
            }
            Err(error) => {
                self.last_error = Some(error.to_string());
                self.last_refresh = Some(Instant::now());
            }
        }
    }

    pub(crate) async fn refresh_agent_picker(&mut self, transport: &HttpTransport) {
        match fetch_agent_picker(transport).await {
            Ok(snapshot) => {
                self.agent_picker = snapshot;
                self.agent_selection.clamp(&self.agent_picker);
                self.last_error = None;
                self.last_refresh = Some(Instant::now());
            }
            Err(error) => {
                self.last_error = Some(error.to_string());
                self.last_refresh = Some(Instant::now());
            }
        }
    }

    pub(crate) async fn open_status(&mut self, transport: &HttpTransport) {
        self.view = AppView::Status;
        self.detail = None;
        self.refresh_status(transport).await;
    }

    pub(crate) async fn open_agent_picker(&mut self, transport: &HttpTransport) {
        self.view = AppView::Agent;
        self.detail = None;
        self.refresh_agent_picker(transport).await;
    }

    pub(crate) fn go_back(&mut self) {
        match self.view {
            AppView::StatusDetail => {
                self.view = AppView::Status;
                self.detail = None;
            }
            AppView::Status | AppView::Agent => {
                self.view = AppView::Chat;
                self.detail = None;
            }
            AppView::Chat => {
                self.chat_input.clear();
            }
        }
    }

    pub(crate) fn select_left(&mut self) {
        match self.view {
            AppView::Status => self.status_selection.move_left(),
            AppView::Agent => self.agent_selection.move_left(),
            AppView::Chat | AppView::StatusDetail => {}
        }
    }

    pub(crate) fn select_right(&mut self) {
        match self.view {
            AppView::Status => self.status_selection.move_right(),
            AppView::Agent => self.agent_selection.move_right(),
            AppView::Chat | AppView::StatusDetail => {}
        }
    }

    pub(crate) fn select_up(&mut self) {
        match self.view {
            AppView::Status => self.status_selection.move_up(&self.snapshot),
            AppView::Agent => self.agent_selection.move_up(&self.agent_picker),
            AppView::Chat => self.scroll_chat_up(1),
            AppView::StatusDetail => {}
        }
    }

    pub(crate) fn select_down(&mut self) {
        match self.view {
            AppView::Status => self.status_selection.move_down(&self.snapshot),
            AppView::Agent => self.agent_selection.move_down(&self.agent_picker),
            AppView::Chat => self.scroll_chat_down(1),
            AppView::StatusDetail => {}
        }
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

    pub(crate) fn enter_current_view(&mut self) {
        match self.view {
            AppView::Status => {
                self.detail = self.status_selection.detail(&self.snapshot);
                if self.detail.is_some() {
                    self.view = AppView::StatusDetail;
                }
            }
            AppView::Agent => self.select_agent_picker_item(),
            AppView::Chat | AppView::StatusDetail => {}
        }
    }

    fn select_agent_picker_item(&mut self) {
        match self.agent_selection.panel {
            AgentPanel::Agents => {
                if let Some(agent) = self.agent_selection.selected_agent(&self.agent_picker) {
                    self.selected_agent = Some(agent.id.clone());
                    self.last_action = Some(format!("selected agent {}", agent.id));
                }
            }
            AgentPanel::Profiles => {
                if let Some(profile) = self.agent_selection.selected_profile(&self.agent_picker) {
                    self.selected_profile = Some(profile.id.clone());
                    self.last_action = Some(format!("selected profile {}", profile.label));
                }
            }
            AgentPanel::Workspaces => {
                if let Some(workspace) = self.agent_selection.selected_workspace(&self.agent_picker)
                {
                    self.selected_workspace = Some(workspace.path.clone());
                    self.last_action = Some(format!("selected workspace {}", workspace.path));
                }
            }
            AgentPanel::Sessions => {
                if let Some(session) = self.agent_selection.selected_session(&self.agent_picker) {
                    self.selected_session = Some(session.session_id.clone());
                    self.last_action = Some(format!("selected session {}", session.session_id));
                }
            }
        }
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
        self.last_error = None;
        self.last_action = None;
        false
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
    Status,
    StatusDetail,
    Agent,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DEFAULT_BASE_URL;
    use crate::render;
    use crate::selection::RuntimePanel;
    use tokio::sync::mpsc;
    use va_client::events::{ChatClientMessage, ChatEvent, ChatSessionAction};
    use va_client::runtime::{
        AgentInfo, ChannelRuntime, ChannelStatus, TunnelRuntime, TunnelStatus,
    };

    fn channel(kind: &str) -> ChannelRuntime {
        ChannelRuntime {
            kind: kind.into(),
            version: Some("0.1.0".into()),
            plugin_dir: None,
            status: ChannelStatus::Running,
            reason: None,
        }
    }

    fn tunnel(provider: &str) -> TunnelRuntime {
        TunnelRuntime {
            provider: provider.into(),
            url: Some(format!("https://{provider}.example.test")),
            status: TunnelStatus::Running,
            uptime_secs: 10,
        }
    }

    #[test]
    fn selects_active_panel_items_with_wrapping_and_clamping() {
        let mut selection = StatusSelection::default();
        let mut snapshot = DashboardSnapshot::default();

        selection.select_next(&snapshot);
        assert_eq!(selection.index(RuntimePanel::Channels), None);

        snapshot.channels = vec![channel("feishu"), channel("discord")];
        selection.clamp(&snapshot);
        assert_eq!(selection.index(RuntimePanel::Channels), Some(0));

        selection.select_next(&snapshot);
        assert_eq!(selection.index(RuntimePanel::Channels), Some(1));

        selection.select_next(&snapshot);
        assert_eq!(selection.index(RuntimePanel::Channels), Some(0));

        selection.select_previous(&snapshot);
        assert_eq!(selection.index(RuntimePanel::Channels), Some(1));

        snapshot.channels.pop();
        selection.clamp(&snapshot);
        assert_eq!(selection.index(RuntimePanel::Channels), Some(0));

        snapshot.channels.clear();
        selection.clamp(&snapshot);
        assert_eq!(selection.index(RuntimePanel::Channels), None);
    }

    #[test]
    fn status_navigation_follows_panel_geometry() {
        let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
        let mut app = TuiApp::new(&endpoint);
        app.view = AppView::Status;
        app.snapshot.channels = vec![channel("feishu")];
        app.snapshot.tunnels = vec![tunnel("cloudflare"), tunnel("ngrok")];
        app.status_selection.clamp(&app.snapshot);

        assert_eq!(app.status_selection.panel, RuntimePanel::Channels);
        assert_eq!(app.status_selection.index(RuntimePanel::Channels), Some(0));

        app.select_down();
        assert_eq!(app.status_selection.panel, RuntimePanel::Tunnels);
        assert_eq!(app.status_selection.index(RuntimePanel::Tunnels), Some(0));
        app.select_down();
        assert_eq!(app.status_selection.index(RuntimePanel::Tunnels), Some(1));

        app.select_up();
        assert_eq!(app.status_selection.panel, RuntimePanel::Channels);
        app.select_right();
        assert_eq!(app.status_selection.panel, RuntimePanel::Agents);
        app.select_down();
        assert_eq!(app.status_selection.panel, RuntimePanel::Sessions);
        app.select_left();
        assert_eq!(app.status_selection.panel, RuntimePanel::Tunnels);
    }

    #[test]
    fn enter_status_item_opens_detail_and_escape_returns() {
        let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
        let mut app = TuiApp::new(&endpoint);
        app.view = AppView::Status;
        app.snapshot.channels = vec![channel("feishu")];
        app.status_selection.clamp(&app.snapshot);

        app.enter_current_view();

        assert_eq!(app.view, AppView::StatusDetail);
        assert_eq!(app.detail.as_ref().unwrap().title, "channel feishu");

        app.go_back();
        assert_eq!(app.view, AppView::Status);
    }

    #[test]
    fn default_view_is_chat() {
        let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
        let app = TuiApp::new(&endpoint);

        assert_eq!(app.view, AppView::Chat);
        assert!(app.chat_messages[0].text.contains("/status"));
    }

    #[test]
    fn chat_arrows_scroll_transcript() {
        let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
        let mut app = TuiApp::new(&endpoint);

        app.select_up();
        app.select_up();
        assert_eq!(app.chat_scroll, 2);
        app.select_down();
        assert_eq!(app.chat_scroll, 1);
        app.follow_chat_tail();
        assert_eq!(app.chat_scroll, 0);
    }

    #[test]
    fn chat_message_send_uses_selected_context() {
        let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
        let mut app = TuiApp::new(&endpoint);
        app.selected_agent = Some("codex".into());
        app.selected_profile = Some("default-profile".into());
        app.selected_session = Some("session-1".into());
        app.selected_workspace = Some("/tmp/work".into());
        let (tx, mut rx) = mpsc::unbounded_channel();

        app.send_chat_message("hello".into(), &tx);

        assert_eq!(
            rx.try_recv().expect("message"),
            ChatClientMessage::Message {
                text: "hello".into(),
                message_id: None,
                agent: Some("codex".into()),
                profile_id: Some("default-profile".into()),
                session_action: Some(ChatSessionAction::Resume),
                session_id: Some("session-1".into()),
                session_workspace: Some("/tmp/work".into()),
                permission_mode: None,
                attachments: Vec::new(),
            }
        );
    }

    #[test]
    fn chat_event_appends_raw_agent_chunks() {
        let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
        let mut app = TuiApp::new(&endpoint);

        app.apply_chat_event(ChatEvent::AcpNotification {
            payload: serde_json::json!({
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": { "text": "hello **raw**" }
                }
            }),
        });
        app.apply_chat_event(ChatEvent::AcpNotification {
            payload: serde_json::json!({
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": { "text": "\nworld" }
                }
            }),
        });

        assert_eq!(app.chat_messages.last().unwrap().role, ChatRole::Response);
        assert_eq!(
            app.chat_messages.last().unwrap().text,
            "hello **raw**\nworld"
        );
    }

    #[test]
    fn tool_updates_change_work_status_without_polluting_transcript() {
        let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
        let mut app = TuiApp::new(&endpoint);
        let initial_count = app.chat_messages.len();

        app.apply_chat_event(ChatEvent::TurnStatus { active: true });
        app.apply_chat_event(ChatEvent::AcpNotification {
            payload: serde_json::json!({
                "update": {
                    "sessionUpdate": "tool_call_update",
                    "toolCall": { "title": "Web Search" },
                    "status": "running"
                }
            }),
        });

        assert_eq!(app.chat_messages.len(), initial_count);
        assert_eq!(
            app.work_status.as_deref(),
            Some("Tool: Web Search (running)")
        );
        assert_eq!(render::view_hint(&app), "Tool: Web Search (running)");
    }

    #[test]
    fn agent_picker_selection_updates_chat_context() {
        let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
        let mut app = TuiApp::new(&endpoint);
        app.view = AppView::Agent;
        app.agent_picker.agents = vec![AgentInfo {
            id: "codex".into(),
            name: "Codex".into(),
            description: "Coding agent".into(),
        }];
        app.agent_selection.clamp(&app.agent_picker);

        app.enter_current_view();

        assert_eq!(app.selected_agent.as_deref(), Some("codex"));
        assert_eq!(app.last_action.as_deref(), Some("selected agent codex"));
    }

    #[test]
    fn exit_requires_second_ctrl_c_within_window() {
        let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
        let mut app = TuiApp::new(&endpoint);
        let start = Instant::now();

        assert!(!app.confirm_exit_request_at(start));
        assert!(app.exit_confirmation_pending());
        assert!(app.exit_confirmation_active_at(start + Duration::from_secs(1)));
        assert!(app.confirm_exit_request_at(start + Duration::from_secs(1)));
        assert!(!app.exit_confirmation_pending());

        assert!(!app.confirm_exit_request_at(start + Duration::from_secs(4)));
        app.clear_expired_exit_confirmation_at(start + Duration::from_secs(7));
        assert!(!app.exit_confirmation_pending());
        assert!(!app.confirm_exit_request_at(start + Duration::from_secs(8)));
    }
}
