use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use clap::Parser;
use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::{Frame, Terminal};
use serde_json::Value;
use va_client::endpoint::ServerEndpoint;
use va_client::http::{HttpMethod, RequestSpec, ResponseSpec};
use va_client::ops;
use va_client::runtime::{
    AgentRuntime, ChannelRuntime, ChannelStatus, TunnelRuntime, TunnelStatus,
};
use va_client::service::ServiceInfoResponse;
use va_client::sessions::SessionListItem;
use va_client::Operation;

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:12358/va";

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

#[derive(Debug)]
struct TuiApp {
    endpoint: String,
    snapshot: DashboardSnapshot,
    selection: SelectionState,
    last_error: Option<String>,
    last_action: Option<String>,
    last_refresh: Option<Instant>,
}

impl TuiApp {
    fn new(endpoint: &ServerEndpoint) -> Self {
        Self {
            endpoint: endpoint.base_url().to_string(),
            snapshot: DashboardSnapshot::default(),
            selection: SelectionState::default(),
            last_error: None,
            last_action: None,
            last_refresh: None,
        }
    }

    async fn refresh(&mut self, transport: &HttpTransport) {
        match fetch_snapshot(transport).await {
            Ok(snapshot) => {
                self.snapshot = snapshot;
                self.clamp_selection();
                self.last_error = None;
                self.last_refresh = Some(Instant::now());
            }
            Err(error) => {
                self.last_error = Some(error.to_string());
                self.last_refresh = Some(Instant::now());
            }
        }
    }

    fn select_next_panel(&mut self) {
        self.selection.active_panel = self.selection.active_panel.next();
    }

    fn select_previous_panel(&mut self) {
        self.selection.active_panel = self.selection.active_panel.previous();
    }

    fn select_next_item(&mut self) {
        self.selection
            .select_next(self.selection.active_panel, self.active_item_count());
    }

    fn select_previous_item(&mut self) {
        self.selection
            .select_previous(self.selection.active_panel, self.active_item_count());
    }

    fn selected_channel_kind(&self) -> Option<String> {
        self.selection
            .index(RuntimePanel::Channels)
            .and_then(|index| self.snapshot.channels.get(index))
            .map(|channel| channel.kind.clone())
    }

    fn selected_tunnel_provider(&self) -> Option<String> {
        self.selection
            .index(RuntimePanel::Tunnels)
            .and_then(|index| self.snapshot.tunnels.get(index))
            .map(|tunnel| tunnel.provider.clone())
    }

    fn selected_agent_route_key(&self) -> Option<String> {
        self.selection
            .index(RuntimePanel::Agents)
            .and_then(|index| self.snapshot.agents.get(index))
            .map(|agent| agent.route_key.clone())
    }

    fn selected_session_id(&self) -> Option<String> {
        self.selection
            .index(RuntimePanel::Sessions)
            .and_then(|index| self.snapshot.sessions.get(index))
            .map(|session| session.session_id.clone())
    }

    async fn run_channel_action(&mut self, transport: &HttpTransport, action: ChannelAction) {
        let Some(kind) = self.selected_channel_kind() else {
            self.last_error = Some("no channel selected".to_string());
            return;
        };

        let result = match action {
            ChannelAction::Start => transport.execute(ops::runtime_start_channel(&kind)).await,
            ChannelAction::Stop => transport.execute(ops::runtime_stop_channel(&kind)).await,
            ChannelAction::Restart => transport.execute(ops::runtime_restart_channel(&kind)).await,
        };

        self.finish_runtime_action(
            result,
            format!("{} channel {kind}", action.label()),
            transport,
        )
        .await;
    }

    async fn run_stop_or_kill_action(&mut self, transport: &HttpTransport) {
        match self.selection.active_panel {
            RuntimePanel::Channels => {
                self.run_channel_action(transport, ChannelAction::Stop)
                    .await;
            }
            RuntimePanel::Tunnels => {
                let Some(provider) = self.selected_tunnel_provider() else {
                    self.last_error = Some("no tunnel selected".to_string());
                    return;
                };
                let result = transport.execute(ops::runtime_kill_tunnel(&provider)).await;
                self.finish_runtime_action(result, format!("killed tunnel {provider}"), transport)
                    .await;
            }
            RuntimePanel::Agents => {
                let Some(route_key) = self.selected_agent_route_key() else {
                    self.last_error = Some("no agent selected".to_string());
                    return;
                };
                let result = transport.execute(ops::runtime_kill_agent(&route_key)).await;
                self.finish_runtime_action(result, format!("killed agent {route_key}"), transport)
                    .await;
            }
            RuntimePanel::Sessions => {
                let Some(session_id) = self.selected_session_id() else {
                    self.last_error = Some("no session selected".to_string());
                    return;
                };
                let result = transport.execute(ops::runtime_kill_pty(&session_id)).await;
                self.finish_runtime_action(result, format!("killed pty {session_id}"), transport)
                    .await;
            }
        }
    }

    async fn finish_runtime_action(
        &mut self,
        result: Result<(), TuiError>,
        action: String,
        transport: &HttpTransport,
    ) {
        match result {
            Ok(()) => {
                self.last_error = None;
                self.last_action = Some(action);
                self.refresh(transport).await;
            }
            Err(error) => {
                self.last_error = Some(error.to_string());
                self.last_action = None;
            }
        }
    }

    fn clamp_selection(&mut self) {
        self.selection
            .clamp(RuntimePanel::Channels, self.snapshot.channels.len());
        self.selection
            .clamp(RuntimePanel::Tunnels, self.snapshot.tunnels.len());
        self.selection
            .clamp(RuntimePanel::Agents, self.snapshot.agents.len());
        self.selection
            .clamp(RuntimePanel::Sessions, self.snapshot.sessions.len());
    }

    fn active_item_count(&self) -> usize {
        match self.selection.active_panel {
            RuntimePanel::Channels => self.snapshot.channels.len(),
            RuntimePanel::Tunnels => self.snapshot.tunnels.len(),
            RuntimePanel::Agents => self.snapshot.agents.len(),
            RuntimePanel::Sessions => self.snapshot.sessions.len(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimePanel {
    Channels,
    Tunnels,
    Agents,
    Sessions,
}

impl RuntimePanel {
    fn next(self) -> Self {
        match self {
            Self::Channels => Self::Tunnels,
            Self::Tunnels => Self::Agents,
            Self::Agents => Self::Sessions,
            Self::Sessions => Self::Channels,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Channels => Self::Sessions,
            Self::Tunnels => Self::Channels,
            Self::Agents => Self::Tunnels,
            Self::Sessions => Self::Agents,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SelectionState {
    active_panel: RuntimePanel,
    channel_index: Option<usize>,
    tunnel_index: Option<usize>,
    agent_index: Option<usize>,
    session_index: Option<usize>,
}

impl Default for SelectionState {
    fn default() -> Self {
        Self {
            active_panel: RuntimePanel::Channels,
            channel_index: None,
            tunnel_index: None,
            agent_index: None,
            session_index: None,
        }
    }
}

impl SelectionState {
    fn index(&self, panel: RuntimePanel) -> Option<usize> {
        match panel {
            RuntimePanel::Channels => self.channel_index,
            RuntimePanel::Tunnels => self.tunnel_index,
            RuntimePanel::Agents => self.agent_index,
            RuntimePanel::Sessions => self.session_index,
        }
    }

    fn set_index(&mut self, panel: RuntimePanel, index: Option<usize>) {
        match panel {
            RuntimePanel::Channels => self.channel_index = index,
            RuntimePanel::Tunnels => self.tunnel_index = index,
            RuntimePanel::Agents => self.agent_index = index,
            RuntimePanel::Sessions => self.session_index = index,
        }
    }

    fn select_next(&mut self, panel: RuntimePanel, item_count: usize) {
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

    fn select_previous(&mut self, panel: RuntimePanel, item_count: usize) {
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

    fn clamp(&mut self, panel: RuntimePanel, item_count: usize) {
        if item_count == 0 {
            self.set_index(panel, None);
            return;
        }
        let index = self.index(panel).unwrap_or(0).min(item_count - 1);
        self.set_index(panel, Some(index));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChannelAction {
    Start,
    Stop,
    Restart,
}

impl ChannelAction {
    fn label(self) -> &'static str {
        match self {
            Self::Start => "started",
            Self::Stop => "stopped",
            Self::Restart => "restarted",
        }
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
    refresh: Duration,
) -> Result<(), TuiError> {
    let (mut terminal, _guard) = enter_terminal()?;
    let mut app = TuiApp::new(&endpoint);
    app.refresh(&transport).await;
    let mut last_tick = Instant::now();

    loop {
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
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Tab | KeyCode::Right => app.select_next_panel(),
                    KeyCode::BackTab | KeyCode::Left => app.select_previous_panel(),
                    KeyCode::Char('j') | KeyCode::Down => app.select_next_item(),
                    KeyCode::Char('k') | KeyCode::Up => app.select_previous_item(),
                    KeyCode::Char('f') => {
                        app.refresh(&transport).await;
                        last_tick = Instant::now();
                    }
                    KeyCode::Char('s') => {
                        app.run_channel_action(&transport, ChannelAction::Start)
                            .await;
                        last_tick = Instant::now();
                    }
                    KeyCode::Char('x') => {
                        app.run_stop_or_kill_action(&transport).await;
                        last_tick = Instant::now();
                    }
                    KeyCode::Char('r') => {
                        app.run_channel_action(&transport, ChannelAction::Restart)
                            .await;
                        last_tick = Instant::now();
                    }
                    _ => {}
                }
            }
        }

        if last_tick.elapsed() >= refresh {
            app.refresh(&transport).await;
            last_tick = Instant::now();
        }
    }

    Ok(())
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

fn render(frame: &mut Frame<'_>, app: &TuiApp) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(5),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(area);

    frame.render_widget(header(app), chunks[0]);
    frame.render_widget(summary(app), chunks[1]);

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[2]);
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
            app.selection.index(RuntimePanel::Channels),
            app.selection.active_panel == RuntimePanel::Channels,
        ),
        left[0],
    );
    frame.render_widget(
        tunnel_list(
            &app.snapshot.tunnels,
            app.selection.index(RuntimePanel::Tunnels),
            app.selection.active_panel == RuntimePanel::Tunnels,
        ),
        left[1],
    );
    frame.render_widget(
        agent_list(
            &app.snapshot.agents,
            app.selection.index(RuntimePanel::Agents),
            app.selection.active_panel == RuntimePanel::Agents,
        ),
        right[0],
    );
    frame.render_widget(
        session_list(
            &app.snapshot.sessions,
            app.selection.index(RuntimePanel::Sessions),
            app.selection.active_panel == RuntimePanel::Sessions,
        ),
        right[1],
    );
    frame.render_widget(footer(app), chunks[3]);
}

fn header(app: &TuiApp) -> Paragraph<'static> {
    let mut lines = vec![Line::from(vec![
        Span::styled(
            "VibeAround",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::raw(app.endpoint.clone()),
    ])];
    if let Some(error) = &app.last_error {
        lines.push(Line::from(Span::styled(
            error.clone(),
            Style::default().fg(Color::Red),
        )));
    }
    Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("va-tui"))
}

fn summary(app: &TuiApp) -> Paragraph<'static> {
    let service = app
        .snapshot
        .service
        .as_ref()
        .map(|service| {
            format!(
                "{} {}  mode={}  port={}",
                service.service, service.version, service.mode, service.port
            )
        })
        .unwrap_or_else(|| "service unavailable".to_string());
    let counts = format!(
        "channels={} tunnels={} agents={} sessions={}",
        app.snapshot.channels.len(),
        app.snapshot.tunnels.len(),
        app.snapshot.agents.len(),
        app.snapshot.sessions.len()
    );
    Paragraph::new(vec![Line::from(service), Line::from(counts)])
        .block(Block::default().borders(Borders::ALL).title("summary"))
}

fn channel_list(
    channels: &[ChannelRuntime],
    selected: Option<usize>,
    active: bool,
) -> List<'static> {
    selectable_list(
        "channels",
        channels.iter().map(channel_line).collect::<Vec<_>>(),
        selected,
        active,
    )
}

fn tunnel_list(tunnels: &[TunnelRuntime], selected: Option<usize>, active: bool) -> List<'static> {
    selectable_list(
        "tunnels",
        tunnels.iter().map(tunnel_line).collect::<Vec<_>>(),
        selected,
        active,
    )
}

fn agent_list(agents: &[AgentRuntime], selected: Option<usize>, active: bool) -> List<'static> {
    selectable_list(
        "agents",
        agents.iter().map(agent_line).collect::<Vec<_>>(),
        selected,
        active,
    )
}

fn session_list(
    sessions: &[SessionListItem],
    selected: Option<usize>,
    active: bool,
) -> List<'static> {
    selectable_list(
        "pty sessions",
        sessions.iter().map(session_line).collect::<Vec<_>>(),
        selected,
        active,
    )
}

fn selectable_list(
    title: &'static str,
    lines: Vec<String>,
    selected: Option<usize>,
    active: bool,
) -> List<'static> {
    let items = if lines.is_empty() {
        vec![ListItem::new("-")]
    } else {
        lines
            .into_iter()
            .enumerate()
            .map(|(index, line)| {
                let marker = if Some(index) == selected { "> " } else { "  " };
                let item = ListItem::new(format!("{marker}{line}"));
                if Some(index) == selected {
                    item.style(
                        Style::default()
                            .fg(if active { Color::Yellow } else { Color::Gray })
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    item
                }
            })
            .collect()
    };
    List::new(items).block(list_block(title, active))
}

fn list_block(title: &'static str, active: bool) -> Block<'static> {
    let block = Block::default().borders(Borders::ALL).title(title);
    if active {
        block
            .border_style(Style::default().fg(Color::Cyan))
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
    } else {
        block
    }
}

fn footer(app: &TuiApp) -> Paragraph<'static> {
    let refreshed = app
        .last_refresh
        .map(|instant| format!("refreshed {}s ago", instant.elapsed().as_secs()))
        .unwrap_or_else(|| "not refreshed yet".to_string());
    let status = app
        .last_error
        .as_ref()
        .map(|error| format!("error: {error}"))
        .or_else(|| {
            app.last_action
                .as_ref()
                .map(|action| format!("last: {action}"))
        })
        .unwrap_or(refreshed);
    Paragraph::new(Line::from(format!(
        "{status} | Tab/Left/Right panel | Up/Down or j/k select | s start channel | x stop/kill | r restart channel | f refresh | q/Esc quit"
    )))
    .block(Block::default().borders(Borders::ALL))
}

fn channel_line(channel: &ChannelRuntime) -> String {
    format!(
        "{}  {}{}",
        channel.kind,
        channel_status_label(channel.status),
        channel
            .reason
            .as_ref()
            .map(|reason| format!("  {reason}"))
            .unwrap_or_default()
    )
}

fn tunnel_line(tunnel: &TunnelRuntime) -> String {
    format!(
        "{}  {}  {}",
        tunnel.provider,
        tunnel_status_label(&tunnel.status),
        tunnel.url.as_deref().unwrap_or("-")
    )
}

fn agent_line(agent: &AgentRuntime) -> String {
    format!(
        "{}  {}  {}",
        agent.route_key,
        if agent.busy { "busy" } else { "idle" },
        agent
            .agent_title
            .as_deref()
            .or(agent.agent_name.as_deref())
            .or(agent.cli_kind.as_deref())
            .unwrap_or("-")
    )
}

fn session_line(session: &SessionListItem) -> String {
    format!(
        "{}  {:?}  {}",
        session.session_id,
        session.tool,
        session.project_path.as_deref().unwrap_or("-")
    )
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

fn tunnel_status_label(status: &TunnelStatus) -> &'static str {
    match status {
        TunnelStatus::Running => "running",
        TunnelStatus::Stopped { .. } => "stopped",
        TunnelStatus::Failed { .. } => "failed",
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
        let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
        let mut app = TuiApp::new(&endpoint);

        app.select_next_item();
        assert_eq!(app.selection.index(RuntimePanel::Channels), None);

        app.snapshot.channels = vec![channel("feishu"), channel("discord")];
        app.clamp_selection();
        assert_eq!(app.selection.index(RuntimePanel::Channels), Some(0));
        assert_eq!(app.selected_channel_kind().as_deref(), Some("feishu"));

        app.select_next_item();
        assert_eq!(app.selection.index(RuntimePanel::Channels), Some(1));
        assert_eq!(app.selected_channel_kind().as_deref(), Some("discord"));

        app.select_next_item();
        assert_eq!(app.selection.index(RuntimePanel::Channels), Some(0));

        app.select_previous_item();
        assert_eq!(app.selection.index(RuntimePanel::Channels), Some(1));

        app.snapshot.channels.pop();
        app.clamp_selection();
        assert_eq!(app.selection.index(RuntimePanel::Channels), Some(0));

        app.snapshot.channels.clear();
        app.clamp_selection();
        assert_eq!(app.selection.index(RuntimePanel::Channels), None);
    }

    #[test]
    fn cycles_panels_and_selects_each_panel_independently() {
        let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
        let mut app = TuiApp::new(&endpoint);
        app.snapshot.channels = vec![channel("feishu")];
        app.snapshot.tunnels = vec![tunnel("cloudflare"), tunnel("ngrok")];
        app.clamp_selection();

        assert_eq!(app.selection.active_panel, RuntimePanel::Channels);
        assert_eq!(app.selection.index(RuntimePanel::Channels), Some(0));

        app.select_next_panel();
        assert_eq!(app.selection.active_panel, RuntimePanel::Tunnels);
        assert_eq!(app.selection.index(RuntimePanel::Tunnels), Some(0));
        app.select_next_item();
        assert_eq!(app.selection.index(RuntimePanel::Tunnels), Some(1));
        assert_eq!(app.selected_tunnel_provider().as_deref(), Some("ngrok"));

        app.select_next_panel();
        assert_eq!(app.selection.active_panel, RuntimePanel::Agents);
        app.select_next_panel();
        assert_eq!(app.selection.active_panel, RuntimePanel::Sessions);
        app.select_next_panel();
        assert_eq!(app.selection.active_panel, RuntimePanel::Channels);

        app.select_previous_panel();
        assert_eq!(app.selection.active_panel, RuntimePanel::Sessions);
    }

    #[test]
    fn channel_action_labels_match_runtime_actions() {
        assert_eq!(ChannelAction::Start.label(), "started");
        assert_eq!(ChannelAction::Stop.label(), "stopped");
        assert_eq!(ChannelAction::Restart.label(), "restarted");
    }

    #[test]
    fn formats_runtime_lines() {
        let channel = channel("feishu");
        assert_eq!(channel_line(&channel), "feishu  running");

        let tunnel = TunnelRuntime {
            provider: "cloudflare".into(),
            url: Some("https://example.test".into()),
            status: TunnelStatus::Running,
            uptime_secs: 10,
        };
        assert_eq!(
            tunnel_line(&tunnel),
            "cloudflare  running  https://example.test"
        );
    }
}
