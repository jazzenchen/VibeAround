use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use futures_util::{SinkExt, StreamExt};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, Paragraph};
use ratatui::{Frame, Terminal};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use va_client::endpoint::ServerEndpoint;
use va_client::events::{
    chat_ws, decode_chat_event, encode_chat_client_message, ChatClientMessage, ChatEvent,
    ChatSessionAction,
};
use va_client::http::{HttpMethod, RequestSpec, ResponseSpec};
use va_client::launcher::LauncherPreferencesResponse;
use va_client::ops;
use va_client::profiles::ModelProfileSummary;
use va_client::runtime::{
    AgentInfo, AgentRuntime, AgentsConfig, ChannelRuntime, ChannelStatus, TunnelRuntime,
    TunnelStatus,
};
use va_client::service::ServiceInfoResponse;
use va_client::sessions::{PtyRunState, SessionListItem};
use va_client::state::ChatState;
use va_client::workspaces::WorkspaceItem;
use va_client::Operation;

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:12358/va";
const BRAND_LOGO: &str = r#" ██╗   ██╗ ██╗ ██████╗  ███████╗  █████╗  ██████╗   ██████╗  ██╗   ██╗ ███╗   ██╗ ██████╗
 ██║   ██║ ██║ ██╔══██╗ ██╔════╝ ██╔══██╗ ██╔══██╗ ██╔═══██╗ ██║   ██║ ████╗  ██║ ██╔══██╗
 ██║   ██║ ██║ ██████╔╝ █████╗   ███████║ ██████╔╝ ██║   ██║ ██║   ██║ ██╔██╗ ██║ ██║  ██║
 ╚██╗ ██╔╝ ██║ ██╔══██╗ ██╔══╝   ██╔══██║ ██╔══██╗ ██║   ██║ ██║   ██║ ██║╚██╗██║ ██║  ██║
  ╚████╔╝  ██║ ██████╔╝ ███████╗ ██║  ██║ ██║  ██║ ╚██████╔╝ ╚██████╔╝ ██║ ╚████║ ██████╔╝
   ╚═══╝   ╚═╝ ╚═════╝  ╚══════╝ ╚═╝  ╚═╝ ╚═╝  ╚═╝  ╚═════╝   ╚═════╝  ╚═╝  ╚═══╝ ╚═════╝"#;
const TAGLINE: &str = "unified runtime for ai coding agents";
const EXIT_CONFIRM_WINDOW: Duration = Duration::from_secs(2);
const BRAND: Color = Color::Cyan;
const OK: Color = Color::Green;
const WARN: Color = Color::Yellow;
const ERROR: Color = Color::Red;
const NEUTRAL: Color = Color::Reset;

#[derive(Debug, Parser)]
#[command(name = "va-tui", version, about = "VibeAround terminal dashboard")]
struct Args {
    #[arg(long)]
    auth_file: Option<PathBuf>,
    #[arg(long)]
    base_url: Option<String>,
    #[arg(long)]
    token: Option<String>,
    #[arg(long, default_value_t = 2000)]
    refresh_ms: u64,
    #[arg(long)]
    once: bool,
}

#[derive(Debug, thiserror::Error)]
enum TuiError {
    #[error("auth is required; pass --token or start VibeAround so auth.json exists at {0}")]
    MissingAuth(String),
    #[error("failed to read auth file {path}: {source}")]
    ReadAuth {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("failed to reach {url}: {source}")]
    Http {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("I/O error while {action}: {source}")]
    Io {
        action: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("client error: {0}")]
    Client(#[from] va_client::ClientError),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

struct HttpTransport {
    endpoint: ServerEndpoint,
    client: reqwest::Client,
}

impl HttpTransport {
    fn new(endpoint: ServerEndpoint) -> Self {
        Self {
            endpoint,
            client: reqwest::Client::new(),
        }
    }

    async fn execute<T>(&self, operation: Operation<T>) -> Result<T, TuiError> {
        let request = operation.request().clone();
        let response = self.send(request).await?;
        Ok(operation.decode(response)?)
    }

    async fn send(&self, request: RequestSpec) -> Result<ResponseSpec, TuiError> {
        let method = match request.method {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Post => reqwest::Method::POST,
            HttpMethod::Put => reqwest::Method::PUT,
            HttpMethod::Delete => reqwest::Method::DELETE,
        };
        let url = self.endpoint.http_url(&request);
        let mut builder = self.client.request(method, &url);
        if let Some(auth) = self.endpoint.authorization_header(&request) {
            builder = builder.header(reqwest::header::AUTHORIZATION, auth);
        }
        if let Some(body) = request.body {
            builder = builder.json(&body);
        }

        let response = builder.send().await.map_err(|source| TuiError::Http {
            url: url.clone(),
            source,
        })?;
        let status = response.status().as_u16();
        let body = response.text().await.map_err(|source| TuiError::Http {
            url: url.clone(),
            source,
        })?;
        let body = if body.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&body).unwrap_or(Value::String(body))
        };
        Ok(ResponseSpec::json(status, body))
    }
}

#[derive(Debug, Default)]
struct DashboardSnapshot {
    service: Option<ServiceInfoResponse>,
    channels: Vec<ChannelRuntime>,
    tunnels: Vec<TunnelRuntime>,
    agents: Vec<AgentRuntime>,
    sessions: Vec<SessionListItem>,
}

#[derive(Debug, Default)]
struct AgentPickerSnapshot {
    agents: Vec<AgentInfo>,
    profiles: Vec<ModelProfileSummary>,
    workspaces: Vec<WorkspaceItem>,
    sessions: Vec<SessionListItem>,
    preferences: Option<LauncherPreferencesResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChatMessage {
    role: ChatRole,
    text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChatRole {
    Notice,
    Request,
    Response,
}

#[derive(Debug)]
enum ChatSocketEvent {
    Connected,
    Closed,
    Error(String),
    Event(ChatEvent),
}

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
    selected_agent: Option<String>,
    selected_profile: Option<String>,
    selected_workspace: Option<String>,
    selected_session: Option<String>,
    detail: Option<DetailContent>,
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
            selected_agent: None,
            selected_profile: None,
            selected_workspace: None,
            selected_session: None,
            detail: None,
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
            AppView::Chat | AppView::StatusDetail => {}
        }
    }

    fn select_down(&mut self) {
        match self.view {
            AppView::Status => self.status_selection.move_down(&self.snapshot),
            AppView::Agent => self.agent_selection.move_down(&self.agent_picker),
            AppView::Chat | AppView::StatusDetail => {}
        }
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
            "/clear" => self.chat_messages.clear(),
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
                self.push_notice(permission_prompt_text(request_id, request));
            }
            ChatEvent::AcpNotification { payload } => {
                self.apply_acp_notification(payload);
            }
            ChatEvent::Error { error } => {
                self.last_error = Some(error.clone());
                self.push_notice(format!("Error: {error}"));
            }
            ChatEvent::PromptDone { .. }
            | ChatEvent::TurnStatus { .. }
            | ChatEvent::SessionMode { .. }
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
                    self.last_action = Some(format!("thinking: {}", one_line(text)));
                }
            }
            Some("tool_call") | Some("tool_call_update") => {
                self.push_notice(tool_activity_text(update));
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct DetailContent {
    title: String,
    lines: Vec<String>,
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
            .draw(|frame| render(frame, &app))
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

async fn run_chat_socket(
    endpoint: ServerEndpoint,
    mut outgoing: mpsc::UnboundedReceiver<ChatClientMessage>,
    incoming: mpsc::UnboundedSender<ChatSocketEvent>,
) {
    let url = endpoint.websocket_url(&chat_ws());
    let (ws, _) = match connect_async(&url).await {
        Ok(connection) => connection,
        Err(error) => {
            let _ = incoming.send(ChatSocketEvent::Error(format!(
                "failed to connect chat websocket: {error}"
            )));
            return;
        }
    };
    let _ = incoming.send(ChatSocketEvent::Connected);
    let (mut ws_tx, mut ws_rx) = ws.split();

    loop {
        tokio::select! {
            Some(message) = outgoing.recv() => {
                let body = match encode_chat_client_message(&message) {
                    Ok(body) => body,
                    Err(error) => {
                        let _ = incoming.send(ChatSocketEvent::Error(format!("failed to encode chat message: {error}")));
                        continue;
                    }
                };
                if let Err(error) = ws_tx.send(Message::Text(body.into())).await {
                    let _ = incoming.send(ChatSocketEvent::Error(format!("failed to send chat message: {error}")));
                    break;
                }
            }
            frame = ws_rx.next() => {
                match frame {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<Value>(&text) {
                            Ok(value) => match decode_chat_event(value) {
                                Ok(event) => {
                                    let _ = incoming.send(ChatSocketEvent::Event(event));
                                }
                                Err(error) => {
                                    let _ = incoming.send(ChatSocketEvent::Error(format!("failed to decode chat event: {error}")));
                                }
                            },
                            Err(error) => {
                                let _ = incoming.send(ChatSocketEvent::Error(format!("failed to parse chat event: {error}")));
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        let _ = incoming.send(ChatSocketEvent::Closed);
                        break;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        let _ = incoming.send(ChatSocketEvent::Error(format!("chat websocket read failed: {error}")));
                        break;
                    }
                }
            }
            else => break,
        }
    }

    let _ = ws_tx.close().await;
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

async fn fetch_snapshot(transport: &HttpTransport) -> Result<DashboardSnapshot, TuiError> {
    Ok(DashboardSnapshot {
        service: Some(transport.execute(ops::service_info()).await?),
        channels: transport.execute(ops::runtime_channels()).await?,
        tunnels: transport.execute(ops::runtime_tunnels()).await?,
        agents: transport.execute(ops::runtime_agent_hosts()).await?,
        sessions: transport.execute(ops::sessions()).await?,
    })
}

async fn fetch_agent_picker(transport: &HttpTransport) -> Result<AgentPickerSnapshot, TuiError> {
    let preferences = transport.execute(ops::launcher_preferences()).await?;
    let agents: AgentsConfig = transport.execute(ops::runtime_agents()).await?;
    Ok(AgentPickerSnapshot {
        agents: agents.agents,
        profiles: transport.execute(ops::model_profiles()).await?,
        workspaces: transport.execute(ops::workspaces()).await?.workspaces,
        sessions: transport.execute(ops::sessions()).await?,
        preferences: Some(preferences),
    })
}

fn content_text(content: Option<&Value>) -> Option<&str> {
    let content = content?;
    content
        .as_str()
        .or_else(|| content.get("text").and_then(Value::as_str))
}

fn permission_prompt_text(request_id: &str, request: &Value) -> String {
    let options = permission_options(request);
    let option_text = if options.is_empty() {
        "no selectable options".to_string()
    } else {
        options
            .iter()
            .map(|option| match option.name.as_deref() {
                Some(name) if name != option.option_id => format!("{name} ({})", option.option_id),
                _ => option.option_id.clone(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "Permission required: {} [{request_id}]. Options: {option_text}. Use /allow <option-id> or /deny.",
        permission_title(request)
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PermissionOption {
    option_id: String,
    name: Option<String>,
}

fn permission_title(request: &Value) -> String {
    request
        .get("toolCall")
        .and_then(|tool_call| {
            value_string_field(tool_call, "title")
                .or_else(|| value_string_field(tool_call, "kind"))
                .or_else(|| value_string_field(tool_call, "name"))
        })
        .or_else(|| value_string_field(request, "title"))
        .unwrap_or_else(|| "Permission requested".into())
}

fn permission_options(request: &Value) -> Vec<PermissionOption> {
    request
        .get("options")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let option_id = value_string_field(item, "optionId")
                        .or_else(|| value_string_field(item, "option_id"))?;
                    Some(PermissionOption {
                        option_id,
                        name: value_string_field(item, "name"),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn tool_activity_text(update: &Value) -> String {
    let tool = update
        .get("toolCall")
        .and_then(|tool_call| {
            value_string_field(tool_call, "title")
                .or_else(|| value_string_field(tool_call, "kind"))
                .or_else(|| value_string_field(tool_call, "name"))
        })
        .or_else(|| value_string_field(update, "title"))
        .or_else(|| value_string_field(update, "toolName"))
        .unwrap_or_else(|| "tool".into());
    let status = value_string_field(update, "status")
        .or_else(|| value_string_field(update, "state"))
        .or_else(|| value_string_field(update, "outcome"));
    match status {
        Some(status) => format!("Tool: {} ({status})", one_line(&tool)),
        None => format!("Tool: {}", one_line(&tool)),
    }
}

fn value_string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn one_line(value: &str) -> String {
    let text = value.split_whitespace().collect::<Vec<_>>().join(" ");
    const LIMIT: usize = 120;
    if text.chars().count() <= LIMIT {
        return text;
    }
    let mut truncated = text.chars().take(LIMIT).collect::<String>();
    truncated.push('…');
    truncated
}

fn render(frame: &mut Frame<'_>, app: &TuiApp) {
    let area = frame.area();
    let brand_mode = brand_mode(area.width, area.height);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(brand_mode.height()),
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(area);

    frame.render_widget(
        brand_header(app, brand_mode, chunks[0].width.saturating_sub(2)),
        chunks[0],
    );
    frame.render_widget(context_strip(app), chunks[1]);
    match app.view {
        AppView::Chat => render_chat_view(frame, app, chunks[2]),
        AppView::Status => render_status_view(frame, app, chunks[2]),
        AppView::StatusDetail => render_status_detail_view(frame, app, chunks[2]),
        AppView::Agent => render_agent_view(frame, app, chunks[2]),
    }
    frame.render_widget(command_bar(app), chunks[3]);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrandMode {
    Narrow,
    Compact,
    FullLogo,
}

impl BrandMode {
    fn height(self) -> u16 {
        match self {
            Self::Narrow => 3,
            Self::Compact => 4,
            Self::FullLogo => 9,
        }
    }
}

fn brand_mode(width: u16, height: u16) -> BrandMode {
    if width >= 96 && height >= 24 {
        BrandMode::FullLogo
    } else if width >= 56 && height >= 14 {
        BrandMode::Compact
    } else {
        BrandMode::Narrow
    }
}

fn brand_header(app: &TuiApp, mode: BrandMode, content_width: u16) -> Paragraph<'static> {
    let content_width = usize::from(content_width);
    let mut lines = Vec::new();
    match mode {
        BrandMode::FullLogo => {
            lines.extend(centered_brand_logo_lines(content_width));
            lines.push(centered_line(
                content_width,
                vec![
                    Span::styled(TAGLINE, muted_style().add_modifier(Modifier::BOLD)),
                    Span::styled("   /   ", muted_style()),
                    Span::raw(app.endpoint.clone()),
                ],
            ));
        }
        BrandMode::Compact => {
            lines.push(centered_line(
                content_width,
                vec![
                    Span::styled(
                        "VibeAround",
                        Style::default().fg(BRAND).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("  terminal runtime console", muted_style()),
                ],
            ));
            lines.push(centered_line(
                content_width,
                vec![Span::raw(app.endpoint.clone())],
            ));
        }
        BrandMode::Narrow => {
            lines.push(centered_line(
                content_width,
                vec![Span::styled(
                    "VA",
                    Style::default().fg(BRAND).add_modifier(Modifier::BOLD),
                )],
            ));
        }
    }

    Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(BRAND)),
    )
}

fn centered_brand_logo_lines(content_width: usize) -> Vec<Line<'static>> {
    let logo_lines = BRAND_LOGO.lines().collect::<Vec<_>>();
    let widths = logo_lines
        .iter()
        .map(|line| Line::from((*line).to_string()).width())
        .collect::<Vec<_>>();
    let block_width = widths.iter().copied().max().unwrap_or(0);
    let left_pad = content_width.saturating_sub(block_width) / 2;

    logo_lines
        .into_iter()
        .zip(widths)
        .map(|(line, width)| {
            Line::from(Span::styled(
                format!(
                    "{}{}{}",
                    " ".repeat(left_pad),
                    line,
                    " ".repeat(block_width.saturating_sub(width))
                ),
                Style::default().fg(BRAND).add_modifier(Modifier::BOLD),
            ))
        })
        .collect()
}

fn centered_line(content_width: usize, spans: Vec<Span<'static>>) -> Line<'static> {
    let line_width = Line::from(spans.clone()).width();
    let left_pad = content_width.saturating_sub(line_width) / 2;
    let mut padded_spans = Vec::with_capacity(spans.len() + 1);
    if left_pad > 0 {
        padded_spans.push(Span::raw(" ".repeat(left_pad)));
    }
    padded_spans.extend(spans);
    Line::from(padded_spans)
}

fn muted_style() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

fn context_strip(app: &TuiApp) -> Paragraph<'static> {
    let spans = match app.view {
        AppView::Chat => chat_context_spans(app),
        AppView::Status | AppView::StatusDetail => status_context_spans(app),
        AppView::Agent => agent_context_spans(app),
    };
    Paragraph::new(Line::from(spans))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
        )
}

fn chat_context_spans(app: &TuiApp) -> Vec<Span<'static>> {
    let session_label = app
        .selected_session
        .as_deref()
        .or(app.chat_state.session_id.as_deref())
        .map(short_id)
        .unwrap_or_else(|| "new".to_string());
    let agent_label = app
        .selected_agent
        .as_deref()
        .or(app.chat_state.default_agent.as_deref())
        .unwrap_or("global");
    let mut spans = vec![Span::styled(
        "chat",
        Style::default().fg(BRAND).add_modifier(Modifier::BOLD),
    )];
    spans.push(separator());
    spans.push(Span::styled(
        if app.chat_connected {
            "connected"
        } else {
            "offline"
        },
        if app.chat_connected {
            Style::default().fg(OK).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(ERROR).add_modifier(Modifier::BOLD)
        },
    ));
    spans.push(separator());
    spans.extend(label_value_spans("agent", agent_label));
    spans.push(Span::raw("  "));
    spans.extend(label_value_spans(
        "profile",
        app.selected_profile.as_deref().unwrap_or("global"),
    ));
    spans.push(Span::raw("  "));
    spans.extend(label_value_spans(
        "workspace",
        app.selected_workspace.as_deref().unwrap_or("global"),
    ));
    spans.push(Span::raw("  "));
    spans.extend(label_value_spans("session", &session_label));
    spans
}

fn status_context_spans(app: &TuiApp) -> Vec<Span<'static>> {
    let service_spans = app
        .snapshot
        .service
        .as_ref()
        .map(|service| {
            vec![
                Span::styled(
                    service.service.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(service.version.clone(), muted_style()),
                Span::raw("  "),
                Span::styled("mode ", muted_style()),
                Span::styled(service.mode.clone(), Style::default().fg(BRAND)),
                Span::raw("  "),
                Span::styled("port ", muted_style()),
                Span::styled(service.port.to_string(), Style::default().fg(WARN)),
            ]
        })
        .unwrap_or_else(|| {
            vec![Span::styled(
                "service unavailable",
                Style::default().fg(ERROR).add_modifier(Modifier::BOLD),
            )]
        });

    let mut spans = service_spans;
    spans.push(separator());
    spans.extend(metric_spans("channels", app.snapshot.channels.len(), BRAND));
    spans.push(Span::raw("  "));
    spans.extend(metric_spans(
        "tunnels",
        app.snapshot.tunnels.len(),
        Color::Magenta,
    ));
    spans.push(Span::raw("  "));
    spans.extend(metric_spans("agents", app.snapshot.agents.len(), OK));
    spans.push(Span::raw("  "));
    spans.extend(metric_spans("sessions", app.snapshot.sessions.len(), WARN));
    spans
}

fn agent_context_spans(app: &TuiApp) -> Vec<Span<'static>> {
    let selected = app
        .agent_picker
        .preferences
        .as_ref()
        .map(|preferences| preferences.selected_agent.as_str())
        .unwrap_or("unknown");
    let mut spans = vec![Span::styled(
        "agent context",
        Style::default().fg(BRAND).add_modifier(Modifier::BOLD),
    )];
    spans.push(separator());
    spans.extend(label_value_spans("default", selected));
    spans.push(Span::raw("  "));
    spans.extend(metric_spans("agents", app.agent_picker.agents.len(), BRAND));
    spans.push(Span::raw("  "));
    spans.extend(metric_spans(
        "profiles",
        app.agent_picker.profiles.len(),
        WARN,
    ));
    spans.push(Span::raw("  "));
    spans.extend(metric_spans(
        "workspaces",
        app.agent_picker.workspaces.len(),
        OK,
    ));
    spans
}

fn label_value_spans(label: &'static str, value: &str) -> Vec<Span<'static>> {
    vec![
        Span::styled(label, muted_style()),
        Span::raw(" "),
        Span::styled(value.to_string(), Style::default()),
    ]
}

fn separator() -> Span<'static> {
    Span::styled("   |   ", muted_style())
}

fn metric_spans(label: &'static str, value: usize, color: Color) -> Vec<Span<'static>> {
    vec![
        Span::styled(label, muted_style()),
        Span::raw(" "),
        Span::styled(
            value.to_string(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ]
}

fn render_chat_view(frame: &mut Frame<'_>, app: &TuiApp, area: ratatui::layout::Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(3)])
        .split(area);
    let messages = if app.chat_messages.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "Type /help for commands.",
            muted_style(),
        )))]
    } else {
        app.chat_messages
            .iter()
            .map(chat_message_item)
            .collect::<Vec<_>>()
    };
    frame.render_widget(
        List::new(messages).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(" chat "),
        ),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("› ", Style::default().fg(BRAND)),
            Span::raw(app.chat_input.clone()),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(BRAND))
                .title(" message "),
        ),
        chunks[1],
    );
}

fn chat_message_item(message: &ChatMessage) -> ListItem<'static> {
    ListItem::new(chat_message_line(message))
}

fn chat_message_line(message: &ChatMessage) -> Line<'static> {
    let (marker, style) = match message.role {
        ChatRole::Notice => ("* ", muted_style()),
        ChatRole::Request => (
            "› ",
            Style::default().fg(BRAND).add_modifier(Modifier::BOLD),
        ),
        ChatRole::Response => ("• ", Style::default()),
    };
    Line::from(vec![
        Span::styled(marker, style),
        Span::raw(message.text.clone()),
    ])
}

fn render_status_view(frame: &mut Frame<'_>, app: &TuiApp, area: ratatui::layout::Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(columns[0]);
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(columns[1]);

    frame.render_widget(
        channel_list(
            &app.snapshot.channels,
            app.status_selection.index(RuntimePanel::Channels),
            app.status_selection.panel == RuntimePanel::Channels,
        ),
        left[0],
    );
    frame.render_widget(
        tunnel_list(
            &app.snapshot.tunnels,
            app.status_selection.index(RuntimePanel::Tunnels),
            app.status_selection.panel == RuntimePanel::Tunnels,
        ),
        left[1],
    );
    frame.render_widget(
        runtime_agent_list(
            &app.snapshot.agents,
            app.status_selection.index(RuntimePanel::Agents),
            app.status_selection.panel == RuntimePanel::Agents,
        ),
        right[0],
    );
    frame.render_widget(
        session_list(
            &app.snapshot.sessions,
            app.status_selection.index(RuntimePanel::Sessions),
            app.status_selection.panel == RuntimePanel::Sessions,
        ),
        right[1],
    );
}

fn render_status_detail_view(frame: &mut Frame<'_>, app: &TuiApp, area: ratatui::layout::Rect) {
    let detail = app.detail.as_ref();
    let title = detail
        .map(|detail| format!(" {} ", detail.title))
        .unwrap_or_else(|| " detail ".to_string());
    let lines = detail
        .map(|detail| {
            detail
                .lines
                .iter()
                .map(|line| Line::from(line.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec![Line::from("No item selected.")]);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(BRAND))
                .title(title),
        ),
        area,
    );
}

fn render_agent_view(frame: &mut Frame<'_>, app: &TuiApp, area: ratatui::layout::Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(columns[0]);
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(columns[1]);

    frame.render_widget(
        picker_list(
            "agents",
            app.agent_picker
                .agents
                .iter()
                .map(agent_info_row)
                .collect::<Vec<_>>(),
            app.agent_selection.index(AgentPanel::Agents),
            app.agent_selection.panel == AgentPanel::Agents,
        ),
        left[0],
    );
    frame.render_widget(
        picker_list(
            "workspaces",
            app.agent_picker
                .workspaces
                .iter()
                .map(workspace_row)
                .collect::<Vec<_>>(),
            app.agent_selection.index(AgentPanel::Workspaces),
            app.agent_selection.panel == AgentPanel::Workspaces,
        ),
        left[1],
    );
    frame.render_widget(
        picker_list(
            "profiles",
            app.agent_picker
                .profiles
                .iter()
                .map(profile_row)
                .collect::<Vec<_>>(),
            app.agent_selection.index(AgentPanel::Profiles),
            app.agent_selection.panel == AgentPanel::Profiles,
        ),
        right[0],
    );
    frame.render_widget(
        picker_list(
            "sessions",
            app.agent_picker
                .sessions
                .iter()
                .map(session_row)
                .collect::<Vec<_>>(),
            app.agent_selection.index(AgentPanel::Sessions),
            app.agent_selection.panel == AgentPanel::Sessions,
        ),
        right[1],
    );
}

fn channel_list(
    channels: &[ChannelRuntime],
    selected: Option<usize>,
    active: bool,
) -> List<'static> {
    selectable_list(
        "channels",
        channels.iter().map(channel_row).collect::<Vec<_>>(),
        selected,
        active,
    )
}

fn tunnel_list(tunnels: &[TunnelRuntime], selected: Option<usize>, active: bool) -> List<'static> {
    selectable_list(
        "tunnels",
        tunnels.iter().map(tunnel_row).collect::<Vec<_>>(),
        selected,
        active,
    )
}

fn runtime_agent_list(
    agents: &[AgentRuntime],
    selected: Option<usize>,
    active: bool,
) -> List<'static> {
    selectable_list(
        "agents",
        agents.iter().map(agent_row).collect::<Vec<_>>(),
        selected,
        active,
    )
}

fn picker_list(
    title: &'static str,
    rows: Vec<Vec<Span<'static>>>,
    selected: Option<usize>,
    active: bool,
) -> List<'static> {
    selectable_list(title, rows, selected, active)
}

fn session_list(
    sessions: &[SessionListItem],
    selected: Option<usize>,
    active: bool,
) -> List<'static> {
    selectable_list(
        "pty sessions",
        sessions.iter().map(session_row).collect::<Vec<_>>(),
        selected,
        active,
    )
}

fn selectable_list(
    title: &'static str,
    rows: Vec<Vec<Span<'static>>>,
    selected: Option<usize>,
    active: bool,
) -> List<'static> {
    let items = if rows.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "  no runtime entries",
            muted_style(),
        )))]
    } else {
        rows.into_iter()
            .enumerate()
            .map(|(index, row)| {
                let marker = if Some(index) == selected { "> " } else { "  " };
                let marker_style = if active {
                    Style::default().fg(WARN)
                } else {
                    muted_style()
                };
                let mut spans = vec![Span::styled(marker, marker_style)];
                spans.extend(row);
                let item = ListItem::new(Line::from(spans));
                if Some(index) == selected {
                    item.style(Style::default().add_modifier(Modifier::BOLD))
                } else {
                    item
                }
            })
            .collect()
    };
    List::new(items).block(list_block(title, active))
}

fn list_block(title: &'static str, active: bool) -> Block<'static> {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(format!(" {title} "));
    if active {
        block
            .border_style(Style::default().fg(BRAND))
            .title_style(Style::default().fg(BRAND).add_modifier(Modifier::BOLD))
    } else {
        block
    }
}

fn command_bar(app: &TuiApp) -> Paragraph<'static> {
    let (status, status_style) = if app.exit_confirmation_pending() {
        (
            "press Ctrl+C again to quit".to_string(),
            Style::default().fg(WARN),
        )
    } else if let Some(error) = &app.last_error {
        (format!("error: {error}"), Style::default().fg(ERROR))
    } else if let Some(action) = &app.last_action {
        (format!("last: {action}"), muted_style())
    } else {
        (view_hint(app), muted_style())
    };
    let mut spans = vec![
        Span::styled(status, status_style),
        Span::styled("  |  ", muted_style()),
    ];
    spans.extend(view_command_spans(app.view));
    spans.extend([
        Span::styled("  |  ", muted_style()),
        key_span("Ctrl+C"),
        Span::raw(" "),
        key_span("Ctrl+C"),
        Span::raw(" quit"),
    ]);
    Paragraph::new(Line::from(spans))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
        )
}

fn view_hint(app: &TuiApp) -> String {
    match app.view {
        AppView::Chat => {
            if app.chat_state.pending_permission_request_id.is_some() {
                "permission pending: /allow <option-id> or /deny".to_string()
            } else if app.chat_state.turn_active {
                "agent is working; /stop to interrupt".to_string()
            } else {
                "type a message or slash command".to_string()
            }
        }
        AppView::Status => app
            .last_refresh
            .map(|instant| format!("status loaded {}s ago", instant.elapsed().as_secs()))
            .unwrap_or_else(|| "status view".to_string()),
        AppView::StatusDetail => "detail view".to_string(),
        AppView::Agent => app
            .last_refresh
            .map(|instant| format!("agent context loaded {}s ago", instant.elapsed().as_secs()))
            .unwrap_or_else(|| "agent context".to_string()),
    }
}

fn view_command_spans(view: AppView) -> Vec<Span<'static>> {
    match view {
        AppView::Chat => vec![
            key_span("Enter"),
            Span::raw(" send  "),
            key_span("/status"),
            Span::raw("  "),
            key_span("/agent"),
            Span::raw("  "),
            key_span("/help"),
        ],
        AppView::Status => vec![
            key_span("Arrows"),
            Span::raw(" move  "),
            key_span("Enter"),
            Span::raw(" detail  "),
            key_span("Esc"),
            Span::raw(" back"),
        ],
        AppView::StatusDetail => vec![key_span("Esc"), Span::raw(" back")],
        AppView::Agent => vec![
            key_span("Arrows"),
            Span::raw(" move  "),
            key_span("Enter"),
            Span::raw(" select  "),
            key_span("Esc"),
            Span::raw(" back"),
        ],
    }
}

fn key_span(value: &'static str) -> Span<'static> {
    Span::styled(
        value,
        Style::default().fg(WARN).add_modifier(Modifier::BOLD),
    )
}

fn channel_row(channel: &ChannelRuntime) -> Vec<Span<'static>> {
    let mut spans = vec![
        Span::styled(
            fixed(&channel.kind, 14),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        status_span(
            channel_status_label(channel.status),
            channel_status_color(channel.status),
            12,
        ),
        Span::styled(
            channel.version.as_deref().unwrap_or("-").to_string(),
            muted_style(),
        ),
    ];
    if let Some(reason) = channel.reason.as_ref().filter(|reason| !reason.is_empty()) {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(reason.clone(), Style::default().fg(ERROR)));
    }
    spans
}

fn tunnel_row(tunnel: &TunnelRuntime) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            fixed(&tunnel.provider, 14),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        status_span(
            tunnel_status_label(&tunnel.status),
            tunnel_status_color(&tunnel.status),
            10,
        ),
        Span::styled(
            tunnel.url.as_deref().unwrap_or("-").to_string(),
            muted_style(),
        ),
    ]
}

fn agent_row(agent: &AgentRuntime) -> Vec<Span<'static>> {
    let name = agent
        .agent_title
        .as_deref()
        .or(agent.agent_name.as_deref())
        .or(agent.cli_kind.as_deref())
        .unwrap_or("-");
    vec![
        Span::styled(
            fixed(&agent.route_key, 18),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        status_span(
            if agent.busy { "busy" } else { "idle" },
            if agent.busy { WARN } else { OK },
            8,
        ),
        Span::styled(name.to_string(), muted_style()),
    ]
}

fn session_row(session: &SessionListItem) -> Vec<Span<'static>> {
    let status = session_status_label(&session.status);
    let tool = format!("{:?}", session.tool).to_ascii_lowercase();
    vec![
        Span::styled(
            fixed(&short_id(&session.session_id), 14),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        status_span(status, session_status_color(&session.status), 10),
        Span::styled(fixed(&tool, 12), muted_style()),
        Span::styled(
            session.project_path.as_deref().unwrap_or("-").to_string(),
            muted_style(),
        ),
    ]
}

fn agent_info_row(agent: &AgentInfo) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            fixed(&agent.id, 14),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(agent.name.clone(), muted_style()),
    ]
}

fn profile_row(profile: &ModelProfileSummary) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            fixed(&profile.label, 18),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(profile.provider_label.clone(), muted_style()),
    ]
}

fn workspace_row(workspace: &WorkspaceItem) -> Vec<Span<'static>> {
    let marker = if workspace.is_default { "* " } else { "  " };
    vec![
        Span::styled(marker, Style::default().fg(BRAND)),
        Span::styled(workspace.path.clone(), Style::default()),
    ]
}

fn fixed(value: &str, width: usize) -> String {
    format!("{value:<width$}")
}

fn status_span(label: &'static str, color: Color, width: usize) -> Span<'static> {
    Span::styled(
        fixed(label, width),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

fn short_id(value: &str) -> String {
    value.chars().take(12).collect()
}

fn channel_status_label(status: ChannelStatus) -> &'static str {
    match status {
        ChannelStatus::NotStarted => "not-started",
        ChannelStatus::Spawning => "spawning",
        ChannelStatus::Running => "running",
        ChannelStatus::Crashed => "crashed",
        ChannelStatus::Stopped => "stopped",
    }
}

fn channel_status_color(status: ChannelStatus) -> Color {
    match status {
        ChannelStatus::Running => OK,
        ChannelStatus::Spawning => WARN,
        ChannelStatus::Crashed => ERROR,
        ChannelStatus::Stopped | ChannelStatus::NotStarted => NEUTRAL,
    }
}

fn tunnel_status_label(status: &TunnelStatus) -> &'static str {
    match status {
        TunnelStatus::Running => "running",
        TunnelStatus::Stopped { .. } => "stopped",
        TunnelStatus::Failed { .. } => "failed",
    }
}

fn tunnel_status_color(status: &TunnelStatus) -> Color {
    match status {
        TunnelStatus::Running => OK,
        TunnelStatus::Stopped { .. } => NEUTRAL,
        TunnelStatus::Failed { .. } => ERROR,
    }
}

fn session_status_label(status: &PtyRunState) -> &'static str {
    match status {
        PtyRunState::Running { .. } => "running",
        PtyRunState::Exited { .. } => "exited",
    }
}

fn session_status_color(status: &PtyRunState) -> Color {
    match status {
        PtyRunState::Running { .. } => OK,
        PtyRunState::Exited { .. } => NEUTRAL,
    }
}

fn channel_detail(channel: &ChannelRuntime) -> DetailContent {
    DetailContent {
        title: format!("channel {}", channel.kind),
        lines: vec![
            format!("kind: {}", channel.kind),
            format!("status: {}", channel_status_label(channel.status)),
            format!("version: {}", channel.version.as_deref().unwrap_or("-")),
            format!(
                "plugin_dir: {}",
                channel.plugin_dir.as_deref().unwrap_or("-")
            ),
            format!("reason: {}", channel.reason.as_deref().unwrap_or("-")),
        ],
    }
}

fn tunnel_detail(tunnel: &TunnelRuntime) -> DetailContent {
    DetailContent {
        title: format!("tunnel {}", tunnel.provider),
        lines: vec![
            format!("provider: {}", tunnel.provider),
            format!("status: {}", tunnel_status_label(&tunnel.status)),
            format!("url: {}", tunnel.url.as_deref().unwrap_or("-")),
            format!("uptime_secs: {}", tunnel.uptime_secs),
        ],
    }
}

fn agent_detail(agent: &AgentRuntime) -> DetailContent {
    DetailContent {
        title: format!("agent {}", agent.route_key),
        lines: vec![
            format!("route_key: {}", agent.route_key),
            format!("channel_kind: {}", agent.channel_kind),
            format!("chat_id: {}", agent.chat_id),
            format!("cli_kind: {}", agent.cli_kind.as_deref().unwrap_or("-")),
            format!("profile: {}", agent.profile.as_deref().unwrap_or("-")),
            format!("session_id: {}", agent.session_id.as_deref().unwrap_or("-")),
            format!("workspace: {}", agent.workspace.as_deref().unwrap_or("-")),
            format!("busy: {}", agent.busy),
            format!("failed: {}", agent.failed.as_deref().unwrap_or("-")),
        ],
    }
}

fn session_detail(session: &SessionListItem) -> DetailContent {
    DetailContent {
        title: format!("session {}", short_id(&session.session_id)),
        lines: vec![
            format!("session_id: {}", session.session_id),
            format!("tool: {:?}", session.tool),
            format!("status: {}", session_status_label(&session.status)),
            format!(
                "project_path: {}",
                session.project_path.as_deref().unwrap_or("-")
            ),
            format!(
                "profile_id: {}",
                session.profile_id.as_deref().unwrap_or("-")
            ),
            format!(
                "profile_label: {}",
                session.profile_label.as_deref().unwrap_or("-")
            ),
            format!(
                "launch_target: {}",
                session.launch_target.as_deref().unwrap_or("-")
            ),
            format!(
                "tmux_session: {}",
                session.tmux_session.as_deref().unwrap_or("-")
            ),
        ],
    }
}

#[cfg(test)]
fn row_text(row: Vec<Span<'static>>) -> String {
    row.into_iter()
        .map(|span| span.content.into_owned())
        .collect::<String>()
        .trim_end()
        .to_string()
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

#[derive(Debug, Default)]
struct RuntimeEnv {
    base_url: Option<String>,
    token: Option<String>,
    auth_file: Option<String>,
    data_dir: Option<String>,
    home_dir: Option<PathBuf>,
}

impl RuntimeEnv {
    fn current() -> Self {
        Self {
            base_url: env_value("VIBEAROUND_BASE_URL"),
            token: env_value("VIBEAROUND_TOKEN").or_else(|| env_value("VIBEAROUND_AUTH_TOKEN")),
            auth_file: env_value("VIBEAROUND_AUTH_FILE"),
            data_dir: env_value("VIBEAROUND_DATA_DIR"),
            home_dir: env::var_os("HOME")
                .or_else(|| env::var_os("USERPROFILE"))
                .map(PathBuf::from),
        }
    }
}

fn resolve_endpoint(args: &Args, runtime_env: &RuntimeEnv) -> Result<ServerEndpoint, TuiError> {
    let base_url = args.base_url.as_deref().or(runtime_env.base_url.as_deref());
    let token = args.token.as_deref().or(runtime_env.token.as_deref());
    let auth_path = auth_file_path(args, runtime_env);

    if let Some(base_url) = base_url {
        let endpoint = ServerEndpoint::new(base_url);
        if let Some(token) = token {
            return Ok(endpoint.with_token(token));
        }
        if auth_path.exists() {
            let auth = read_auth_file(&auth_path)?;
            return Ok(endpoint.with_token(auth.token));
        }
        return Err(TuiError::MissingAuth(auth_path.display().to_string()));
    }

    if let Some(token) = token {
        return Ok(ServerEndpoint::new(DEFAULT_BASE_URL).with_token(token));
    }

    if auth_path.exists() {
        let auth = read_auth_file(&auth_path)?;
        return Ok(ServerEndpoint::from_auth_file(&auth));
    }

    Err(TuiError::MissingAuth(auth_path.display().to_string()))
}

fn read_auth_file(path: &Path) -> Result<va_client::auth::AuthFile, TuiError> {
    let body = fs::read_to_string(path).map_err(|source| TuiError::ReadAuth {
        path: path.display().to_string(),
        source,
    })?;
    va_client::auth::parse_auth_file(&body).map_err(TuiError::from)
}

fn auth_file_path(args: &Args, runtime_env: &RuntimeEnv) -> PathBuf {
    args.auth_file
        .clone()
        .unwrap_or_else(|| default_auth_path(runtime_env))
}

fn default_auth_path(runtime_env: &RuntimeEnv) -> PathBuf {
    if let Some(path) = &runtime_env.auth_file {
        return PathBuf::from(path);
    }
    if let Some(path) = &runtime_env.data_dir {
        return PathBuf::from(path).join("auth.json");
    }
    runtime_env
        .home_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".vibearound")
        .join("auth.json")
}

fn env_value(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
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
    fn resolves_base_url_with_auth_file_token() {
        let path = std::env::temp_dir().join(format!(
            "va-tui-auth-test-{}-{}.json",
            std::process::id(),
            line!()
        ));
        let _ = fs::remove_file(&path);
        fs::write(&path, r#"{ "port": 12358, "token": "secret" }"#).expect("write auth");
        let args = Args {
            auth_file: Some(path.clone()),
            base_url: Some("http://localhost:9000/va".into()),
            token: None,
            refresh_ms: 2000,
            once: false,
        };

        let endpoint = resolve_endpoint(&args, &RuntimeEnv::default()).expect("endpoint");

        assert_eq!(endpoint.base_url(), "http://localhost:9000/va");
        assert_eq!(endpoint.token(), Some("secret"));

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn default_auth_path_uses_env_shape() {
        let env = RuntimeEnv {
            data_dir: Some("/tmp/va".into()),
            home_dir: Some(PathBuf::from("/home/test")),
            ..Default::default()
        };

        assert_eq!(default_auth_path(&env), PathBuf::from("/tmp/va/auth.json"));
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
    fn chat_items_use_terminal_markers_without_role_labels() {
        let request = row_text(
            chat_message_line(&ChatMessage {
                role: ChatRole::Request,
                text: "hello".into(),
            })
            .spans,
        );
        let response = row_text(
            chat_message_line(&ChatMessage {
                role: ChatRole::Response,
                text: "hi".into(),
            })
            .spans,
        );
        let notice = row_text(
            chat_message_line(&ChatMessage {
                role: ChatRole::Notice,
                text: "ready".into(),
            })
            .spans,
        );

        assert_eq!(request, "› hello");
        assert_eq!(response, "• hi");
        assert_eq!(notice, "* ready");
        assert!(!request.contains("you"));
        assert!(!notice.contains("system"));
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
    fn permission_prompt_lists_allow_command_options() {
        let text = permission_prompt_text(
            "req-1",
            &serde_json::json!({
                "toolCall": { "title": "Read" },
                "options": [
                    { "optionId": "allow-once", "name": "Allow" },
                    { "optionId": "reject", "name": "Reject" }
                ]
            }),
        );

        assert!(text.contains("Permission required: Read"));
        assert!(text.contains("Allow (allow-once)"));
        assert!(text.contains("/allow <option-id>"));
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
    fn brand_mode_scales_with_terminal_size() {
        assert_eq!(brand_mode(40, 24), BrandMode::Narrow);
        assert_eq!(brand_mode(80, 18), BrandMode::Compact);
        assert_eq!(brand_mode(96, 24), BrandMode::FullLogo);
        assert_eq!(BrandMode::Narrow.height(), 3);
        assert_eq!(BrandMode::FullLogo.height(), 9);
    }

    #[test]
    fn centered_brand_logo_lines_share_one_block_width() {
        let lines = centered_brand_logo_lines(120);
        let widths = lines.iter().map(Line::width).collect::<Vec<_>>();

        assert_eq!(lines.len(), BRAND_LOGO.lines().count());
        assert!(widths.iter().all(|width| *width == widths[0]));
        assert!(widths[0] <= 120);
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

    #[test]
    fn formats_runtime_lines() {
        let channel = channel("feishu");
        assert_eq!(
            row_text(channel_row(&channel)),
            "feishu        running     0.1.0"
        );

        let tunnel = TunnelRuntime {
            provider: "cloudflare".into(),
            url: Some("https://example.test".into()),
            status: TunnelStatus::Running,
            uptime_secs: 10,
        };
        assert_eq!(
            row_text(tunnel_row(&tunnel)),
            "cloudflare    running   https://example.test"
        );

        let session = SessionListItem {
            session_id: "abcdef1234567890".into(),
            tool: va_client::sessions::PtyTool::Codex,
            status: PtyRunState::Running {
                tool: va_client::sessions::PtyTool::Codex,
            },
            created_at: 1,
            project_path: Some("/tmp/project".into()),
            profile_id: None,
            profile_label: None,
            launch_target: None,
            tmux_session: None,
        };
        assert_eq!(
            row_text(session_row(&session)),
            "abcdef123456  running   codex       /tmp/project"
        );
    }
}
