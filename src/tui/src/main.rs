use std::io;
use std::time::{Duration, Instant};

use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use serde_json::Value;
use tokio::sync::mpsc;
use va_client::endpoint::ServerEndpoint;
use va_client::events::{ChatClientMessage, ChatEvent, ChatSessionAction};
use va_client::profiles::ModelProfileSummary;
use va_client::runtime::AgentInfo;
#[cfg(test)]
use va_client::runtime::{ChannelRuntime, ChannelStatus, TunnelRuntime, TunnelStatus};
use va_client::sessions::SessionListItem;
use va_client::state::ChatState;
use va_client::workspaces::WorkspaceItem;

mod chat;
mod chat_socket;
mod config;
mod data;
mod render;
mod theme;
mod transport;

use chat::{
    content_text, one_line, permission_prompt_text, tool_activity_text, ChatMessage, ChatRole,
};
use chat_socket::{run_chat_socket, ChatSocketEvent};
#[cfg(test)]
use config::DEFAULT_BASE_URL;
use config::{resolve_endpoint, Args, RuntimeEnv};
use data::{fetch_agent_picker, fetch_snapshot, AgentPickerSnapshot, DashboardSnapshot};
use render::{agent_detail, channel_detail, session_detail, tunnel_detail, DetailContent};
use transport::{HttpTransport, TuiError};

const EXIT_CONFIRM_WINDOW: Duration = Duration::from_secs(2);

#[derive(Debug)]
struct TuiApp {
    endpoint: String,
    view: AppView,
    chat_state: ChatState,
    chat_connected: bool,
    snapshot: DashboardSnapshot,
    agent_picker: AgentPickerSnapshot,
    status_selection: StatusSelection,
    agent_selection: AgentSelection,
    chat_messages: Vec<ChatMessage>,
    chat_input: String,
    chat_scroll: usize,
    selected_agent: Option<String>,
    selected_profile: Option<String>,
    selected_workspace: Option<String>,
    selected_session: Option<String>,
    detail: Option<DetailContent>,
    work_status: Option<String>,
    last_error: Option<String>,
    last_action: Option<String>,
    last_refresh: Option<Instant>,
    exit_confirmation_started: Option<Instant>,
}

impl TuiApp {
    fn new(endpoint: &ServerEndpoint) -> Self {
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

    async fn refresh_status(&mut self, transport: &HttpTransport) {
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

    async fn refresh_agent_picker(&mut self, transport: &HttpTransport) {
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

    async fn open_status(&mut self, transport: &HttpTransport) {
        self.view = AppView::Status;
        self.detail = None;
        self.refresh_status(transport).await;
    }

    async fn open_agent_picker(&mut self, transport: &HttpTransport) {
        self.view = AppView::Agent;
        self.detail = None;
        self.refresh_agent_picker(transport).await;
    }

    fn go_back(&mut self) {
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

    fn select_left(&mut self) {
        match self.view {
            AppView::Status => self.status_selection.move_left(),
            AppView::Agent => self.agent_selection.move_left(),
            AppView::Chat | AppView::StatusDetail => {}
        }
    }

    fn select_right(&mut self) {
        match self.view {
            AppView::Status => self.status_selection.move_right(),
            AppView::Agent => self.agent_selection.move_right(),
            AppView::Chat | AppView::StatusDetail => {}
        }
    }

    fn select_up(&mut self) {
        match self.view {
            AppView::Status => self.status_selection.move_up(&self.snapshot),
            AppView::Agent => self.agent_selection.move_up(&self.agent_picker),
            AppView::Chat => self.scroll_chat_up(1),
            AppView::StatusDetail => {}
        }
    }

    fn select_down(&mut self) {
        match self.view {
            AppView::Status => self.status_selection.move_down(&self.snapshot),
            AppView::Agent => self.agent_selection.move_down(&self.agent_picker),
            AppView::Chat => self.scroll_chat_down(1),
            AppView::StatusDetail => {}
        }
    }

    fn scroll_chat_up(&mut self, lines: usize) {
        self.chat_scroll = self.chat_scroll.saturating_add(lines);
    }

    fn scroll_chat_down(&mut self, lines: usize) {
        self.chat_scroll = self.chat_scroll.saturating_sub(lines);
    }

    fn follow_chat_tail(&mut self) {
        self.chat_scroll = 0;
    }

    fn enter_current_view(&mut self) {
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

    async fn submit_chat_input(
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

    fn send_chat_message(
        &mut self,
        text: String,
        chat_tx: &mpsc::UnboundedSender<ChatClientMessage>,
    ) {
        let message = ChatClientMessage::Message {
            text,
            message_id: None,
            agent: self.selected_agent.clone(),
            profile_id: self.selected_profile.clone(),
            session_action: self
                .selected_session
                .as_ref()
                .map(|_| ChatSessionAction::Resume),
            session_id: self.selected_session.clone(),
            session_workspace: self.selected_workspace.clone(),
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

    fn apply_chat_socket_event(&mut self, event: ChatSocketEvent) {
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

    fn apply_chat_event(&mut self, event: ChatEvent) {
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

    fn confirm_exit_request(&mut self) -> bool {
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

    fn clear_expired_exit_confirmation(&mut self) {
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

    fn exit_confirmation_pending(&self) -> bool {
        self.exit_confirmation_started.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppView {
    Chat,
    Status,
    StatusDetail,
    Agent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimePanel {
    Channels,
    Tunnels,
    Agents,
    Sessions,
}

impl RuntimePanel {
    fn left(self) -> Self {
        match self {
            Self::Agents => Self::Channels,
            Self::Sessions => Self::Tunnels,
            Self::Channels | Self::Tunnels => self,
        }
    }

    fn right(self) -> Self {
        match self {
            Self::Channels => Self::Agents,
            Self::Tunnels => Self::Sessions,
            Self::Agents | Self::Sessions => self,
        }
    }

    fn up(self) -> Self {
        match self {
            Self::Tunnels => Self::Channels,
            Self::Sessions => Self::Agents,
            Self::Channels | Self::Agents => self,
        }
    }

    fn down(self) -> Self {
        match self {
            Self::Channels => Self::Tunnels,
            Self::Agents => Self::Sessions,
            Self::Tunnels | Self::Sessions => self,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StatusSelection {
    panel: RuntimePanel,
    channel_index: Option<usize>,
    tunnel_index: Option<usize>,
    agent_index: Option<usize>,
    session_index: Option<usize>,
}

impl Default for StatusSelection {
    fn default() -> Self {
        Self {
            panel: RuntimePanel::Channels,
            channel_index: None,
            tunnel_index: None,
            agent_index: None,
            session_index: None,
        }
    }
}

impl StatusSelection {
    fn index(&self, panel: RuntimePanel) -> Option<usize> {
        match panel {
            RuntimePanel::Channels => self.channel_index,
            RuntimePanel::Tunnels => self.tunnel_index,
            RuntimePanel::Agents => self.agent_index,
            RuntimePanel::Sessions => self.session_index,
        }
    }

    fn active_index(&self) -> Option<usize> {
        self.index(self.panel)
    }

    fn set_index(&mut self, panel: RuntimePanel, index: Option<usize>) {
        match panel {
            RuntimePanel::Channels => self.channel_index = index,
            RuntimePanel::Tunnels => self.tunnel_index = index,
            RuntimePanel::Agents => self.agent_index = index,
            RuntimePanel::Sessions => self.session_index = index,
        }
    }

    fn move_left(&mut self) {
        self.panel = self.panel.left();
    }

    fn move_right(&mut self) {
        self.panel = self.panel.right();
    }

    fn move_up(&mut self, snapshot: &DashboardSnapshot) {
        if self.panel.up() == self.panel {
            self.select_previous(snapshot);
        } else {
            self.panel = self.panel.up();
        }
    }

    fn move_down(&mut self, snapshot: &DashboardSnapshot) {
        if self.panel.down() == self.panel {
            self.select_next(snapshot);
        } else {
            self.panel = self.panel.down();
        }
    }

    fn select_next(&mut self, snapshot: &DashboardSnapshot) {
        let panel = self.panel;
        self.select_next_in_panel(panel, self.item_count(snapshot, panel));
    }

    fn select_previous(&mut self, snapshot: &DashboardSnapshot) {
        let panel = self.panel;
        self.select_previous_in_panel(panel, self.item_count(snapshot, panel));
    }

    fn select_next_in_panel(&mut self, panel: RuntimePanel, item_count: usize) {
        if item_count == 0 {
            self.set_index(panel, None);
            return;
        }
        let next = self
            .index(panel)
            .map(|index| (index + 1) % item_count)
            .unwrap_or(0);
        self.set_index(panel, Some(next));
    }

    fn select_previous_in_panel(&mut self, panel: RuntimePanel, item_count: usize) {
        if item_count == 0 {
            self.set_index(panel, None);
            return;
        }
        let last = item_count - 1;
        let previous = self
            .index(panel)
            .map(|index| if index == 0 { last } else { index - 1 })
            .unwrap_or(0);
        self.set_index(panel, Some(previous));
    }

    fn clamp(&mut self, snapshot: &DashboardSnapshot) {
        for panel in [
            RuntimePanel::Channels,
            RuntimePanel::Tunnels,
            RuntimePanel::Agents,
            RuntimePanel::Sessions,
        ] {
            self.clamp_panel(panel, self.item_count(snapshot, panel));
        }
    }

    fn clamp_panel(&mut self, panel: RuntimePanel, item_count: usize) {
        if item_count == 0 {
            self.set_index(panel, None);
            return;
        }
        let index = self.index(panel).unwrap_or(0).min(item_count - 1);
        self.set_index(panel, Some(index));
    }

    fn item_count(&self, snapshot: &DashboardSnapshot, panel: RuntimePanel) -> usize {
        match panel {
            RuntimePanel::Channels => snapshot.channels.len(),
            RuntimePanel::Tunnels => snapshot.tunnels.len(),
            RuntimePanel::Agents => snapshot.agents.len(),
            RuntimePanel::Sessions => snapshot.sessions.len(),
        }
    }

    fn detail(&self, snapshot: &DashboardSnapshot) -> Option<DetailContent> {
        match self.panel {
            RuntimePanel::Channels => self
                .active_index()
                .and_then(|index| snapshot.channels.get(index))
                .map(channel_detail),
            RuntimePanel::Tunnels => self
                .active_index()
                .and_then(|index| snapshot.tunnels.get(index))
                .map(tunnel_detail),
            RuntimePanel::Agents => self
                .active_index()
                .and_then(|index| snapshot.agents.get(index))
                .map(agent_detail),
            RuntimePanel::Sessions => self
                .active_index()
                .and_then(|index| snapshot.sessions.get(index))
                .map(session_detail),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentPanel {
    Agents,
    Profiles,
    Workspaces,
    Sessions,
}

impl AgentPanel {
    fn left(self) -> Self {
        match self {
            Self::Profiles => Self::Agents,
            Self::Sessions => Self::Workspaces,
            Self::Agents | Self::Workspaces => self,
        }
    }

    fn right(self) -> Self {
        match self {
            Self::Agents => Self::Profiles,
            Self::Workspaces => Self::Sessions,
            Self::Profiles | Self::Sessions => self,
        }
    }

    fn up(self) -> Self {
        match self {
            Self::Workspaces => Self::Agents,
            Self::Sessions => Self::Profiles,
            Self::Agents | Self::Profiles => self,
        }
    }

    fn down(self) -> Self {
        match self {
            Self::Agents => Self::Workspaces,
            Self::Profiles => Self::Sessions,
            Self::Workspaces | Self::Sessions => self,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentSelection {
    panel: AgentPanel,
    agent_index: Option<usize>,
    profile_index: Option<usize>,
    workspace_index: Option<usize>,
    session_index: Option<usize>,
}

impl Default for AgentSelection {
    fn default() -> Self {
        Self {
            panel: AgentPanel::Agents,
            agent_index: None,
            profile_index: None,
            workspace_index: None,
            session_index: None,
        }
    }
}

impl AgentSelection {
    fn index(&self, panel: AgentPanel) -> Option<usize> {
        match panel {
            AgentPanel::Agents => self.agent_index,
            AgentPanel::Profiles => self.profile_index,
            AgentPanel::Workspaces => self.workspace_index,
            AgentPanel::Sessions => self.session_index,
        }
    }

    fn set_index(&mut self, panel: AgentPanel, index: Option<usize>) {
        match panel {
            AgentPanel::Agents => self.agent_index = index,
            AgentPanel::Profiles => self.profile_index = index,
            AgentPanel::Workspaces => self.workspace_index = index,
            AgentPanel::Sessions => self.session_index = index,
        }
    }

    fn move_left(&mut self) {
        self.panel = self.panel.left();
    }

    fn move_right(&mut self) {
        self.panel = self.panel.right();
    }

    fn move_up(&mut self, snapshot: &AgentPickerSnapshot) {
        if self.panel.up() == self.panel {
            self.select_previous(snapshot);
        } else {
            self.panel = self.panel.up();
        }
    }

    fn move_down(&mut self, snapshot: &AgentPickerSnapshot) {
        if self.panel.down() == self.panel {
            self.select_next(snapshot);
        } else {
            self.panel = self.panel.down();
        }
    }

    fn select_next(&mut self, snapshot: &AgentPickerSnapshot) {
        let panel = self.panel;
        self.select_next_in_panel(panel, self.item_count(snapshot, panel));
    }

    fn select_previous(&mut self, snapshot: &AgentPickerSnapshot) {
        let panel = self.panel;
        self.select_previous_in_panel(panel, self.item_count(snapshot, panel));
    }

    fn select_next_in_panel(&mut self, panel: AgentPanel, item_count: usize) {
        if item_count == 0 {
            self.set_index(panel, None);
            return;
        }
        let next = self
            .index(panel)
            .map(|index| (index + 1) % item_count)
            .unwrap_or(0);
        self.set_index(panel, Some(next));
    }

    fn select_previous_in_panel(&mut self, panel: AgentPanel, item_count: usize) {
        if item_count == 0 {
            self.set_index(panel, None);
            return;
        }
        let last = item_count - 1;
        let previous = self
            .index(panel)
            .map(|index| if index == 0 { last } else { index - 1 })
            .unwrap_or(0);
        self.set_index(panel, Some(previous));
    }

    fn clamp(&mut self, snapshot: &AgentPickerSnapshot) {
        for panel in [
            AgentPanel::Agents,
            AgentPanel::Profiles,
            AgentPanel::Workspaces,
            AgentPanel::Sessions,
        ] {
            self.clamp_panel(panel, self.item_count(snapshot, panel));
        }
    }

    fn clamp_panel(&mut self, panel: AgentPanel, item_count: usize) {
        if item_count == 0 {
            self.set_index(panel, None);
            return;
        }
        let index = self.index(panel).unwrap_or(0).min(item_count - 1);
        self.set_index(panel, Some(index));
    }

    fn item_count(&self, snapshot: &AgentPickerSnapshot, panel: AgentPanel) -> usize {
        match panel {
            AgentPanel::Agents => snapshot.agents.len(),
            AgentPanel::Profiles => snapshot.profiles.len(),
            AgentPanel::Workspaces => snapshot.workspaces.len(),
            AgentPanel::Sessions => snapshot.sessions.len(),
        }
    }

    fn selected_agent<'a>(&self, snapshot: &'a AgentPickerSnapshot) -> Option<&'a AgentInfo> {
        self.agent_index
            .and_then(|index| snapshot.agents.get(index))
    }

    fn selected_profile<'a>(
        &self,
        snapshot: &'a AgentPickerSnapshot,
    ) -> Option<&'a ModelProfileSummary> {
        self.profile_index
            .and_then(|index| snapshot.profiles.get(index))
    }

    fn selected_workspace<'a>(
        &self,
        snapshot: &'a AgentPickerSnapshot,
    ) -> Option<&'a WorkspaceItem> {
        self.workspace_index
            .and_then(|index| snapshot.workspaces.get(index))
    }

    fn selected_session<'a>(
        &self,
        snapshot: &'a AgentPickerSnapshot,
    ) -> Option<&'a SessionListItem> {
        self.session_index
            .and_then(|index| snapshot.sessions.get(index))
    }
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("va-tui: {error}");
        std::process::exit(2);
    }
}

async fn run() -> Result<(), TuiError> {
    let args = Args::parse();
    let endpoint = resolve_endpoint(&args, &RuntimeEnv::current())?;
    let transport = HttpTransport::new(endpoint.clone());
    if args.once {
        let snapshot = fetch_snapshot(&transport).await?;
        print_once(&endpoint, &snapshot);
        return Ok(());
    }
    run_dashboard(
        endpoint,
        transport,
        Duration::from_millis(args.refresh_ms.max(250)),
    )
    .await
}

async fn run_dashboard(
    endpoint: ServerEndpoint,
    transport: HttpTransport,
    _refresh: Duration,
) -> Result<(), TuiError> {
    let (mut terminal, _guard) = enter_terminal()?;
    let mut app = TuiApp::new(&endpoint);
    let (chat_tx, chat_rx) = mpsc::unbounded_channel::<ChatClientMessage>();
    let (socket_event_tx, mut socket_event_rx) = mpsc::unbounded_channel::<ChatSocketEvent>();
    let chat_task = tokio::spawn(run_chat_socket(endpoint.clone(), chat_rx, socket_event_tx));

    loop {
        while let Ok(event) = socket_event_rx.try_recv() {
            app.apply_chat_socket_event(event);
        }
        app.clear_expired_exit_confirmation();
        terminal
            .draw(|frame| render::render(frame, &app))
            .map_err(|source| TuiError::Io {
                action: "drawing terminal dashboard",
                source,
            })?;

        if event::poll(Duration::from_millis(100)).map_err(|source| TuiError::Io {
            action: "polling terminal events",
            source,
        })? {
            if let Event::Key(key) = event::read().map_err(|source| TuiError::Io {
                action: "reading terminal events",
                source,
            })? {
                if is_ctrl_c(&key) {
                    if app.confirm_exit_request() {
                        break;
                    }
                    continue;
                }

                match key.code {
                    KeyCode::Esc => app.go_back(),
                    KeyCode::Left => app.select_left(),
                    KeyCode::Right => app.select_right(),
                    KeyCode::Up => app.select_up(),
                    KeyCode::Down => app.select_down(),
                    KeyCode::PageUp if app.view == AppView::Chat => app.scroll_chat_up(10),
                    KeyCode::PageDown if app.view == AppView::Chat => app.scroll_chat_down(10),
                    KeyCode::Enter => match app.view {
                        AppView::Chat => app.submit_chat_input(&transport, &chat_tx).await,
                        AppView::Status | AppView::Agent => app.enter_current_view(),
                        AppView::StatusDetail => {}
                    },
                    KeyCode::Backspace if app.view == AppView::Chat => {
                        app.chat_input.pop();
                    }
                    KeyCode::Char(ch)
                        if app.view == AppView::Chat
                            && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        app.chat_input.push(ch);
                    }
                    _ => {}
                }
            }
        }
    }

    chat_task.abort();
    Ok(())
}

fn is_ctrl_c(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
        && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn enter_terminal() -> Result<(Terminal<CrosstermBackend<io::Stdout>>, TerminalGuard), TuiError> {
    enable_raw_mode().map_err(|source| TuiError::Io {
        action: "enabling raw mode",
        source,
    })?;
    let mut stdout = io::stdout();
    if let Err(source) = execute!(stdout, EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(TuiError::Io {
            action: "entering alternate screen",
            source,
        });
    }
    match Terminal::new(CrosstermBackend::new(stdout)) {
        Ok(terminal) => Ok((terminal, TerminalGuard)),
        Err(source) => {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
            Err(TuiError::Io {
                action: "creating terminal",
                source,
            })
        }
    }
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

fn print_once(endpoint: &ServerEndpoint, snapshot: &DashboardSnapshot) {
    println!("endpoint: {}", endpoint.base_url());
    if let Some(service) = &snapshot.service {
        println!(
            "service: {} {} mode={} port={}",
            service.service, service.version, service.mode, service.port
        );
    }
    println!(
        "channels: {} tunnels: {} agents: {} sessions: {}",
        snapshot.channels.len(),
        snapshot.tunnels.len(),
        snapshot.agents.len(),
        snapshot.sessions.len()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn recognizes_ctrl_c_as_exit_key() {
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let plain_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE);
        let ctrl_q = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);

        assert!(is_ctrl_c(&ctrl_c));
        assert!(!is_ctrl_c(&plain_c));
        assert!(!is_ctrl_c(&ctrl_q));
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
