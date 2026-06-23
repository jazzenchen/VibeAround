use super::*;
use std::collections::BTreeMap;

use crate::config::DEFAULT_BASE_URL;
use crate::render;
use crate::runtime_socket::RuntimeSocketEvent;
use crate::selection::{AgentPanel, RuntimePanel};
use ratatui::backend::TestBackend;
use ratatui::layout::Position;
use ratatui::Terminal;
use serde_json::Value;
use tokio::sync::mpsc;
use va_client::events::{ChatClientMessage, ChatEvent, ChatSessionAction};
use va_client::launcher::{LauncherAgentPreferenceSummary, LauncherPreferencesResponse};
use va_client::profiles::{AuthMode, ModelProfileSummary};
use va_client::runtime::{
    AgentInfo, AgentRuntime, ChannelRuntime, ChannelStatus, TunnelRuntime, TunnelStatus,
};
use va_client::sessions::{LaunchSessionInfo, PtyRunState, PtyTool, SessionListItem};
use va_client::workspaces::WorkspaceItem;

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

fn agent(id: &str) -> AgentInfo {
    AgentInfo {
        id: id.into(),
        name: id.into(),
        description: format!("{id} agent"),
    }
}

fn runtime_agent(route_key: &str) -> AgentRuntime {
    AgentRuntime {
        route_key: route_key.into(),
        channel_kind: "tui".into(),
        chat_id: route_key.into(),
        attached_routes: Vec::new(),
        cli_kind: Some("codex".into()),
        profile: None,
        profile_label: None,
        session_id: None,
        workspace: Some("/tmp/project".into()),
        busy: false,
        failed: None,
        started_at: 0,
        agent_name: Some("Codex".into()),
        agent_title: None,
        agent_version: Some("1.0.0".into()),
        multi_agent_turns: Vec::new(),
        subagents: Vec::new(),
    }
}

fn profile(id: &str) -> ModelProfileSummary {
    ModelProfileSummary {
        id: id.into(),
        label: id.into(),
        provider: "provider".into(),
        provider_label: "Provider".into(),
        provider_icon: None,
        auth_mode: AuthMode::ApiKey,
        api_types: vec!["chat".into()],
        launch_targets: Vec::new(),
        api_type_warnings: BTreeMap::new(),
        api_type_models: BTreeMap::new(),
        api_type_model_options: BTreeMap::new(),
        api_type_headers: BTreeMap::new(),
    }
}

fn workspace(path: &str) -> WorkspaceItem {
    WorkspaceItem {
        path: path.into(),
        is_default: false,
        is_builtin: false,
    }
}

fn session(session_id: &str, project_path: &str, profile_id: &str) -> SessionListItem {
    SessionListItem {
        session_id: session_id.into(),
        tool: PtyTool::Codex,
        status: PtyRunState::Running {
            tool: PtyTool::Codex,
        },
        created_at: 1,
        project_path: Some(project_path.into()),
        profile_id: Some(profile_id.into()),
        profile_label: Some(profile_id.into()),
        launch_target: None,
        tmux_session: None,
    }
}

fn launch_session(session_id: &str, agent_id: &str, workspace: &str) -> LaunchSessionInfo {
    LaunchSessionInfo {
        agent_id: agent_id.into(),
        session_id: session_id.into(),
        title: format!("{agent_id} session"),
        workspace: workspace.into(),
        updated_at: 1,
        short_id: session_id.chars().take(8).collect(),
        archived: false,
        active: false,
    }
}

fn launcher_preferences(
    selected_agent: &str,
    profile_id: Option<&str>,
    workspace: Option<&str>,
) -> LauncherPreferencesResponse {
    let mut agent_preferences = BTreeMap::new();
    agent_preferences.insert(
        selected_agent.to_string(),
        LauncherAgentPreferenceSummary {
            profile_id: profile_id.map(str::to_string),
            workspace: workspace.map(str::to_string),
            executable_path: None,
            launch_args: Value::Null,
        },
    );
    LauncherPreferencesResponse {
        selected_agent: selected_agent.into(),
        default_agent: selected_agent.into(),
        default_profile_id: Some("default-profile".into()),
        enabled_agents: vec![selected_agent.into()],
        agent_preferences,
        local_agent_api_enabled: true,
        profile_connections: Value::Null,
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
fn runtime_socket_events_update_status_snapshot_and_clamp_selection() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let mut app = TuiApp::new(&endpoint);
    app.view = AppView::Status;
    app.snapshot.channels = vec![channel("feishu"), channel("discord")];
    app.snapshot.tunnels = vec![tunnel("cloudflare")];
    app.snapshot.agents = vec![runtime_agent("tui:chat-1")];
    app.status_selection.clamp(&app.snapshot);
    app.status_selection.select_next(&app.snapshot);
    assert_eq!(app.status_selection.index(RuntimePanel::Channels), Some(1));
    app.last_error = Some("old runtime error".into());

    app.apply_runtime_socket_event(RuntimeSocketEvent::Channels(vec![channel("feishu")]));

    assert_eq!(app.snapshot.channels.len(), 1);
    assert_eq!(app.snapshot.channels[0].kind, "feishu");
    assert_eq!(app.status_selection.index(RuntimePanel::Channels), Some(0));
    assert_eq!(app.last_error, None);
    assert!(app.last_refresh.is_some());

    app.apply_runtime_socket_event(RuntimeSocketEvent::Tunnels(vec![tunnel("ngrok")]));
    assert_eq!(app.snapshot.tunnels[0].provider, "ngrok");

    app.apply_runtime_socket_event(RuntimeSocketEvent::Agents(vec![runtime_agent(
        "tui:chat-2",
    )]));
    assert_eq!(app.snapshot.agents[0].route_key, "tui:chat-2");

    app.apply_runtime_socket_event(RuntimeSocketEvent::Sessions(vec![session(
        "session-1",
        "/tmp/session",
        "session-profile",
    )]));
    assert_eq!(app.snapshot.sessions[0].session_id, "session-1");

    app.apply_runtime_socket_event(RuntimeSocketEvent::Error("runtime socket closed".into()));
    assert_eq!(app.last_error.as_deref(), Some("runtime socket closed"));
}

#[test]
fn pty_session_socket_does_not_mutate_agent_picker_sessions() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let mut app = TuiApp::new(&endpoint);
    app.view = AppView::Agent;
    app.agent_picker.sessions = vec![launch_session("launch-1", "codex", "/tmp/launch")];
    app.selected_session = Some("launch-1".into());
    app.agent_selection.panel = AgentPanel::Sessions;
    app.agent_selection.clamp(&app.agent_picker);
    assert_eq!(app.agent_selection.index(AgentPanel::Sessions), Some(0));

    app.apply_runtime_socket_event(RuntimeSocketEvent::Sessions(vec![session(
        "new-session",
        "/tmp/new",
        "new-profile",
    )]));

    assert_eq!(app.agent_picker.sessions[0].session_id, "launch-1");
    assert_eq!(app.snapshot.sessions[0].session_id, "new-session");
    assert_eq!(app.agent_selection.index(AgentPanel::Sessions), Some(0));
    assert_eq!(app.selected_session.as_deref(), Some("launch-1"));
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
fn status_render_uses_minimal_semantic_boundaries() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let mut app = TuiApp::new(&endpoint);
    app.view = AppView::Status;
    app.snapshot.channels = vec![channel("feishu")];
    app.snapshot.tunnels = vec![tunnel("cloudflare")];
    app.snapshot.agents = vec![runtime_agent("tui:chat-1")];
    app.snapshot.sessions = vec![session("session-1", "/tmp/session", "profile-1")];
    app.status_selection.clamp(&app.snapshot);

    let backend = TestBackend::new(100, 32);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| render::render(frame, &app))
        .expect("draw");
    let screen = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<Vec<_>>()
        .join("");

    assert!(screen.contains("channels"));
    assert!(screen.contains("feishu"));
    assert!(screen.contains('│'));
    assert!(screen.contains('─'));
    assert!(!screen.contains('┌'));
    assert!(!screen.contains('┐'));
    assert!(!screen.contains('└'));
    assert!(!screen.contains('┘'));
}

#[test]
fn default_view_is_chat() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let app = TuiApp::new(&endpoint);

    assert_eq!(app.view, AppView::Chat);
    assert!(app.chat_messages[0].text.contains("/status"));
}

#[test]
fn chat_render_uses_conversation_markers_without_panel_box() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let mut app = TuiApp::new(&endpoint);
    app.chat_connected = true;
    app.chat_messages = vec![
        ChatMessage {
            role: ChatRole::Request,
            text: "hello".into(),
        },
        ChatMessage {
            role: ChatRole::Response,
            text: "hi there".into(),
        },
    ];

    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| render::render(frame, &app))
        .expect("draw");
    let screen = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<Vec<_>>()
        .join("");

    assert!(screen.contains("› hello"));
    assert!(screen.contains("• hi there"));
    assert_eq!(screen.matches("chat").count(), 1);
    assert!(screen.contains('─'));
    assert!(!screen.contains('┌'));
    assert!(!screen.contains('┐'));
    assert!(!screen.contains('└'));
    assert!(!screen.contains('┘'));
}

#[test]
fn chat_render_shows_multiline_input_with_continuation_indent() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let mut app = TuiApp::new(&endpoint);
    app.set_chat_input_for_test("first line\nsecond line");

    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| render::render(frame, &app))
        .expect("draw");
    let screen = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<Vec<_>>()
        .join("");

    assert!(screen.contains("› first line"));
    assert!(screen.contains("  second line"));
    terminal
        .backend_mut()
        .assert_cursor_position(Position::new(13, 19));
}

#[test]
fn effective_chat_context_prefers_local_selection_over_launcher_preferences() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let mut app = TuiApp::new(&endpoint);
    app.agent_picker.preferences = Some(launcher_preferences(
        "codex",
        Some("codex-profile"),
        Some("/tmp/codex"),
    ));

    assert_eq!(app.effective_agent(), Some("codex"));
    assert_eq!(app.effective_profile(), Some("codex-profile"));
    assert_eq!(app.effective_workspace(), Some("/tmp/codex"));
    assert_eq!(app.effective_session(), None);

    app.selected_agent = Some("claude".into());
    app.selected_profile = Some("claude-profile".into());
    app.selected_workspace = Some("/tmp/claude".into());
    app.selected_session = Some("session-1".into());

    assert_eq!(app.effective_agent(), Some("claude"));
    assert_eq!(app.effective_profile(), Some("claude-profile"));
    assert_eq!(app.effective_workspace(), Some("/tmp/claude"));
    assert_eq!(app.effective_session(), Some("session-1"));
}

#[test]
fn chat_arrows_scroll_transcript() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let mut app = TuiApp::new(&endpoint);

    app.select_up();
    app.select_up();
    assert_eq!(app.chat_scroll, 2);
    assert_eq!(
        render::view_hint(&app),
        "scrollback 2 lines; Down/PageDown moves toward latest"
    );
    app.select_down();
    assert_eq!(app.chat_scroll, 1);
    app.follow_chat_tail();
    assert_eq!(app.chat_scroll, 0);
}

#[test]
fn chat_input_editing_supports_paste_multiline_and_word_delete() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let mut app = TuiApp::new(&endpoint);

    app.insert_chat_text("hello\r\nworld  ");
    assert_eq!(app.chat_input, "hello\nworld  ");
    assert_eq!(app.chat_cursor, app.chat_input.len());

    app.delete_chat_word();
    assert_eq!(app.chat_input, "hello\n");
    assert_eq!(app.chat_cursor, app.chat_input.len());

    app.insert_chat_text("again");
    app.insert_chat_newline();
    app.insert_chat_text("tail");
    assert_eq!(app.chat_input, "hello\nagain\ntail");

    app.delete_chat_char();
    assert_eq!(app.chat_input, "hello\nagain\ntai");

    app.clear_chat_input();
    assert!(app.chat_input.is_empty());
    assert_eq!(app.chat_cursor, 0);
}

#[test]
fn chat_input_cursor_edits_inside_text() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let mut app = TuiApp::new(&endpoint);

    app.insert_chat_text("hello");
    app.move_chat_cursor_left();
    app.move_chat_cursor_left();
    app.insert_chat_text("X");

    assert_eq!(app.chat_input, "helXlo");
    assert_eq!(app.chat_cursor, 4);

    app.delete_chat_char();
    assert_eq!(app.chat_input, "hello");
    assert_eq!(app.chat_cursor, 3);

    app.delete_chat_forward_char();
    assert_eq!(app.chat_input, "helo");
    assert_eq!(app.chat_cursor, 3);

    app.move_chat_cursor_start();
    app.insert_chat_text("> ");
    app.move_chat_cursor_end();
    app.insert_chat_text(" <");
    assert_eq!(app.chat_input, "> helo <");
    assert_eq!(app.chat_cursor, app.chat_input.len());
}

#[test]
fn chat_input_cursor_respects_unicode_boundaries() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let mut app = TuiApp::new(&endpoint);

    app.insert_chat_text("你好吗");
    app.move_chat_cursor_left();
    app.delete_chat_forward_char();

    assert_eq!(app.chat_input, "你好");
    assert_eq!(app.chat_cursor, "你好".len());

    app.move_chat_cursor_left();
    app.insert_chat_text("也");
    assert_eq!(app.chat_input, "你也好");
}

#[test]
fn chat_input_delete_word_preserves_text_after_cursor() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let mut app = TuiApp::new(&endpoint);

    app.set_chat_input_for_test("hello brave world");
    app.set_chat_cursor_for_test("hello brave".len());
    app.delete_chat_word();

    assert_eq!(app.chat_input, "hello world");
    assert_eq!(app.chat_cursor, "hello ".len());
}

#[test]
fn chat_input_delete_to_end_preserves_text_before_cursor() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let mut app = TuiApp::new(&endpoint);

    app.set_chat_input_for_test("hello brave world");
    app.set_chat_cursor_for_test("hello".len());
    app.delete_chat_to_end();

    assert_eq!(app.chat_input, "hello");
    assert_eq!(app.chat_cursor, "hello".len());
}

#[test]
fn chat_input_cursor_moves_by_word() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let mut app = TuiApp::new(&endpoint);

    app.set_chat_input_for_test("hello brave world");
    app.move_chat_cursor_word_left();
    assert_eq!(app.chat_cursor, "hello brave ".len());

    app.move_chat_cursor_word_left();
    assert_eq!(app.chat_cursor, "hello ".len());

    app.move_chat_cursor_word_right();
    assert_eq!(app.chat_cursor, "hello brave".len());

    app.move_chat_cursor_word_right();
    assert_eq!(app.chat_cursor, "hello brave world".len());
}

#[test]
fn chat_input_word_cursor_respects_unicode_boundaries() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let mut app = TuiApp::new(&endpoint);

    app.set_chat_input_for_test("你好 世界 again");
    app.move_chat_cursor_word_left();
    assert_eq!(app.chat_cursor, "你好 世界 ".len());

    app.move_chat_cursor_word_left();
    assert_eq!(app.chat_cursor, "你好 ".len());

    app.move_chat_cursor_word_right();
    assert_eq!(app.chat_cursor, "你好 世界".len());
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
fn chat_message_send_uses_effective_launcher_context() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let mut app = TuiApp::new(&endpoint);
    app.agent_picker.preferences = Some(launcher_preferences(
        "codex",
        Some("codex-profile"),
        Some("/tmp/codex"),
    ));
    let (tx, mut rx) = mpsc::unbounded_channel();

    app.send_chat_message("hello".into(), &tx);

    assert_eq!(
        rx.try_recv().expect("message"),
        ChatClientMessage::Message {
            text: "hello".into(),
            message_id: None,
            agent: Some("codex".into()),
            profile_id: Some("codex-profile".into()),
            session_action: None,
            session_id: None,
            session_workspace: Some("/tmp/codex".into()),
            permission_mode: None,
            attachments: Vec::new(),
        }
    );
}

#[tokio::test]
async fn slash_new_prepares_next_message_for_new_session() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let transport = HttpTransport::new(ServerEndpoint::new(DEFAULT_BASE_URL));
    let mut app = TuiApp::new(&endpoint);
    app.selected_agent = Some("codex".into());
    app.selected_profile = Some("default-profile".into());
    app.selected_workspace = Some("/tmp/work".into());
    app.selected_session = Some("session-1".into());
    app.chat_state.session_id = Some("session-1".into());
    app.chat_input = "/new".into();
    let (tx, mut rx) = mpsc::unbounded_channel();

    app.submit_chat_input(&transport, &tx).await;

    assert!(rx.try_recv().is_err());
    assert_eq!(app.selected_session, None);
    assert_eq!(app.chat_state.session_id, None);
    assert!(app.force_new_session);
    assert_eq!(render::view_hint(&app), "next message starts a new session");

    app.send_chat_message("hello".into(), &tx);

    assert_eq!(
        rx.try_recv().expect("message"),
        ChatClientMessage::Message {
            text: "hello".into(),
            message_id: None,
            agent: Some("codex".into()),
            profile_id: Some("default-profile".into()),
            session_action: Some(ChatSessionAction::New),
            session_id: None,
            session_workspace: Some("/tmp/work".into()),
            permission_mode: None,
            attachments: Vec::new(),
        }
    );
    assert!(!app.force_new_session);
}

#[tokio::test]
async fn unknown_slash_command_is_forwarded_to_agent() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let transport = HttpTransport::new(ServerEndpoint::new(DEFAULT_BASE_URL));
    let mut app = TuiApp::new(&endpoint);
    app.selected_agent = Some("codex".into());
    app.chat_input = "/review current changes".into();
    let (tx, mut rx) = mpsc::unbounded_channel();

    app.submit_chat_input(&transport, &tx).await;

    assert_eq!(
        rx.try_recv().expect("forwarded slash command"),
        ChatClientMessage::Message {
            text: "/review current changes".into(),
            message_id: None,
            agent: Some("codex".into()),
            profile_id: None,
            session_action: None,
            session_id: None,
            session_workspace: None,
            permission_mode: None,
            attachments: Vec::new(),
        }
    );
    assert_eq!(app.chat_messages.last().unwrap().role, ChatRole::Request);
    assert_eq!(
        app.chat_messages.last().unwrap().text,
        "/review current changes"
    );
}

#[tokio::test]
async fn submit_chat_input_sends_multiline_prompt() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let transport = HttpTransport::new(ServerEndpoint::new(DEFAULT_BASE_URL));
    let mut app = TuiApp::new(&endpoint);
    app.set_chat_input_for_test(" first line\nsecond line\n");
    let (tx, mut rx) = mpsc::unbounded_channel();

    app.submit_chat_input(&transport, &tx).await;

    assert_eq!(
        rx.try_recv().expect("message"),
        ChatClientMessage::Message {
            text: " first line\nsecond line\n".into(),
            message_id: None,
            agent: None,
            profile_id: None,
            session_action: None,
            session_id: None,
            session_workspace: None,
            permission_mode: None,
            attachments: Vec::new(),
        }
    );
    assert!(app.chat_input.is_empty());
    assert_eq!(app.chat_cursor, 0);
    assert_eq!(
        app.chat_messages.last().unwrap().text,
        " first line\nsecond line\n"
    );
}

#[tokio::test]
async fn failed_chat_submit_restores_input_without_transcript_echo() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let transport = HttpTransport::new(ServerEndpoint::new(DEFAULT_BASE_URL));
    let mut app = TuiApp::new(&endpoint);
    app.set_chat_input_for_test("do not lose this");
    let initial_messages = app.chat_messages.len();
    let (tx, rx) = mpsc::unbounded_channel();
    drop(rx);

    app.submit_chat_input(&transport, &tx).await;

    assert_eq!(app.chat_input, "do not lose this");
    assert_eq!(app.chat_cursor, app.chat_input.len());
    assert_eq!(app.chat_messages.len(), initial_messages);
    assert_eq!(
        app.last_error.as_deref(),
        Some("chat websocket task is not running")
    );
}

#[tokio::test]
async fn slash_command_submission_trims_command_boundary_whitespace() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let transport = HttpTransport::new(ServerEndpoint::new(DEFAULT_BASE_URL));
    let mut app = TuiApp::new(&endpoint);
    app.set_chat_input_for_test("  /mode accept  ");
    let (tx, mut rx) = mpsc::unbounded_channel();

    app.submit_chat_input(&transport, &tx).await;

    assert_eq!(
        rx.try_recv().expect("mode command"),
        ChatClientMessage::SetMode {
            mode_id: "acceptEdits".into(),
        }
    );
}

#[test]
fn new_session_intent_survives_failed_send() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let mut app = TuiApp::new(&endpoint);
    app.force_new_session = true;
    let (tx, rx) = mpsc::unbounded_channel();
    drop(rx);

    app.send_chat_message("hello".into(), &tx);

    assert!(app.force_new_session);
    assert_eq!(
        app.last_error.as_deref(),
        Some("chat websocket task is not running")
    );
}

#[tokio::test]
async fn slash_mode_sends_set_mode_command() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let transport = HttpTransport::new(ServerEndpoint::new(DEFAULT_BASE_URL));
    let mut app = TuiApp::new(&endpoint);
    app.chat_input = "/mode accept".into();
    let (tx, mut rx) = mpsc::unbounded_channel();

    app.submit_chat_input(&transport, &tx).await;

    assert_eq!(
        rx.try_recv().expect("mode command"),
        ChatClientMessage::SetMode {
            mode_id: "acceptEdits".into(),
        }
    );
    assert_eq!(
        app.last_action.as_deref(),
        Some("requested mode acceptEdits")
    );
}

#[tokio::test]
async fn slash_help_renders_multiline_command_reference() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let transport = HttpTransport::new(ServerEndpoint::new(DEFAULT_BASE_URL));
    let mut app = TuiApp::new(&endpoint);
    app.chat_input = "/help".into();
    app.chat_scroll = 3;
    let (tx, mut rx) = mpsc::unbounded_channel();

    app.submit_chat_input(&transport, &tx).await;

    assert!(rx.try_recv().is_err());
    assert_eq!(app.chat_scroll, 0);
    let help = &app.chat_messages.last().unwrap().text;
    assert!(help.contains("Commands\n/status runtime status"));
    assert!(help.contains("/agent agent, profile, workspace, session"));
    assert!(help.contains("Shift+Enter newline"));
    assert!(help.contains("Alt+Left/Right word"));
    assert!(help.contains("Ctrl+A/E start/end"));
    assert!(help.contains("Ctrl+K delete tail"));
}

#[tokio::test]
async fn slash_mode_lists_dynamic_session_mode_options_without_sending() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let transport = HttpTransport::new(ServerEndpoint::new(DEFAULT_BASE_URL));
    let mut app = TuiApp::new(&endpoint);
    app.chat_state.session_mode = Some(serde_json::json!({
        "source": "session_mode",
        "name": "Permission mode",
        "currentValue": "default",
        "options": [
            { "value": "default", "name": "Default" },
            { "value": "acceptEdits", "name": "Accept edits" }
        ]
    }));
    app.chat_input = "/mode".into();
    let (tx, mut rx) = mpsc::unbounded_channel();

    app.submit_chat_input(&transport, &tx).await;

    assert!(rx.try_recv().is_err());
    let notice = &app.chat_messages.last().unwrap().text;
    assert!(notice.contains("Permission mode\n1 Default (default) *"));
    assert!(notice.contains("2 Accept edits (acceptEdits)"));
    assert!(notice.contains("Use /mode <number|value>."));
}

#[tokio::test]
async fn slash_mode_uses_config_option_source_when_present() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let transport = HttpTransport::new(ServerEndpoint::new(DEFAULT_BASE_URL));
    let mut app = TuiApp::new(&endpoint);
    app.chat_state.session_mode = Some(serde_json::json!({
        "source": "config_option",
        "configId": "permissions",
        "currentValue": "default",
        "options": [
            { "value": "default", "name": "Default" },
            { "value": "acceptEdits", "name": "Accept edits" }
        ]
    }));
    app.chat_input = "/mode 2".into();
    let (tx, mut rx) = mpsc::unbounded_channel();

    app.submit_chat_input(&transport, &tx).await;

    assert_eq!(
        rx.try_recv().expect("config option"),
        ChatClientMessage::SetConfigOption {
            config_id: "permissions".into(),
            value: "acceptEdits".into(),
        }
    );
    assert_eq!(
        app.last_action.as_deref(),
        Some("requested mode acceptEdits")
    );
}

#[tokio::test]
async fn slash_mode_accepts_dynamic_multi_word_option_name() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let transport = HttpTransport::new(ServerEndpoint::new(DEFAULT_BASE_URL));
    let mut app = TuiApp::new(&endpoint);
    app.chat_state.session_mode = Some(serde_json::json!({
        "source": "config_option",
        "configId": "permissions",
        "currentValue": "default",
        "options": [
            { "value": "default", "name": "Default" },
            { "value": "acceptEdits", "name": "Accept edits" }
        ]
    }));
    app.chat_input = "/mode Accept edits".into();
    let (tx, mut rx) = mpsc::unbounded_channel();

    app.submit_chat_input(&transport, &tx).await;

    assert_eq!(
        rx.try_recv().expect("config option"),
        ChatClientMessage::SetConfigOption {
            config_id: "permissions".into(),
            value: "acceptEdits".into(),
        }
    );
}

#[tokio::test]
async fn slash_mode_uses_session_mode_source_when_present() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let transport = HttpTransport::new(ServerEndpoint::new(DEFAULT_BASE_URL));
    let mut app = TuiApp::new(&endpoint);
    app.chat_state.session_mode = Some(serde_json::json!({
        "source": "session_mode",
        "currentValue": "default",
        "options": [
            { "value": "default", "name": "Default" },
            { "value": "bypassPermissions", "name": "Bypass permissions" }
        ]
    }));
    app.chat_input = "/mode bypassPermissions".into();
    let (tx, mut rx) = mpsc::unbounded_channel();

    app.submit_chat_input(&transport, &tx).await;

    assert_eq!(
        rx.try_recv().expect("mode command"),
        ChatClientMessage::SetMode {
            mode_id: "bypassPermissions".into(),
        }
    );
}

#[tokio::test]
async fn slash_mode_rejects_unknown_mode_without_sending() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let transport = HttpTransport::new(ServerEndpoint::new(DEFAULT_BASE_URL));
    let mut app = TuiApp::new(&endpoint);
    app.chat_input = "/mode turbo".into();
    app.chat_scroll = 2;
    let (tx, mut rx) = mpsc::unbounded_channel();

    app.submit_chat_input(&transport, &tx).await;

    assert!(rx.try_recv().is_err());
    assert_eq!(app.chat_scroll, 0);
    assert!(app
        .chat_messages
        .last()
        .unwrap()
        .text
        .contains("Unknown mode"));
}

#[tokio::test]
async fn slash_allow_defaults_to_first_non_reject_permission_option() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let transport = HttpTransport::new(ServerEndpoint::new(DEFAULT_BASE_URL));
    let mut app = TuiApp::new(&endpoint);
    app.apply_chat_event(ChatEvent::PermissionRequest {
        request_id: "req-1".into(),
        request: serde_json::json!({
            "toolCall": { "title": "Read" },
            "options": [
                { "optionId": "reject", "name": "Reject", "kind": "reject" },
                { "optionId": "allow-once", "name": "Allow" }
            ]
        }),
    });
    assert_eq!(
        render::view_hint(&app),
        "permission pending: /allow [number|option-id] or /deny"
    );
    app.chat_input = "/allow".into();
    let (tx, mut rx) = mpsc::unbounded_channel();

    app.submit_chat_input(&transport, &tx).await;

    assert_eq!(
        rx.try_recv().expect("permission response"),
        ChatClientMessage::permission_selected("req-1", "allow-once")
    );
    assert_eq!(app.chat_state.pending_permission_request_id, None);
    assert_eq!(app.chat_state.pending_permission, None);
    assert_eq!(
        app.last_action.as_deref(),
        Some("permission selected: allow-once")
    );
}

#[tokio::test]
async fn slash_allow_accepts_numbered_permission_option() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let transport = HttpTransport::new(ServerEndpoint::new(DEFAULT_BASE_URL));
    let mut app = TuiApp::new(&endpoint);
    app.apply_chat_event(ChatEvent::PermissionRequest {
        request_id: "req-2".into(),
        request: serde_json::json!({
            "toolCall": { "title": "Write" },
            "options": [
                { "optionId": "allow-once", "name": "Allow" },
                { "optionId": "allow-always", "name": "Always allow" },
                { "optionId": "reject", "name": "Reject", "kind": "reject" }
            ]
        }),
    });
    app.chat_input = "/allow 2".into();
    let (tx, mut rx) = mpsc::unbounded_channel();

    app.submit_chat_input(&transport, &tx).await;

    assert_eq!(
        rx.try_recv().expect("permission response"),
        ChatClientMessage::permission_selected("req-2", "allow-always")
    );
    assert_eq!(app.chat_state.pending_permission_request_id, None);
    assert_eq!(
        app.last_action.as_deref(),
        Some("permission selected: allow-always")
    );
}

#[tokio::test]
async fn slash_allow_unknown_permission_option_keeps_pending_request() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let transport = HttpTransport::new(ServerEndpoint::new(DEFAULT_BASE_URL));
    let mut app = TuiApp::new(&endpoint);
    app.apply_chat_event(ChatEvent::PermissionRequest {
        request_id: "req-3".into(),
        request: serde_json::json!({
            "toolCall": { "title": "Read" },
            "options": [{ "optionId": "allow-once", "name": "Allow" }]
        }),
    });
    app.chat_input = "/allow 9".into();
    let (tx, mut rx) = mpsc::unbounded_channel();

    app.submit_chat_input(&transport, &tx).await;

    assert!(rx.try_recv().is_err());
    assert_eq!(
        app.chat_state.pending_permission_request_id.as_deref(),
        Some("req-3")
    );
    assert!(app
        .chat_messages
        .last()
        .unwrap()
        .text
        .contains("Unknown permission option"));
}

#[tokio::test]
async fn slash_deny_clears_pending_permission_after_send() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let transport = HttpTransport::new(ServerEndpoint::new(DEFAULT_BASE_URL));
    let mut app = TuiApp::new(&endpoint);
    app.apply_chat_event(ChatEvent::PermissionRequest {
        request_id: "req-4".into(),
        request: serde_json::json!({
            "toolCall": { "title": "Read" },
            "options": [{ "optionId": "allow-once", "name": "Allow" }]
        }),
    });
    app.chat_input = "/deny".into();
    let (tx, mut rx) = mpsc::unbounded_channel();

    app.submit_chat_input(&transport, &tx).await;

    assert_eq!(
        rx.try_recv().expect("permission response"),
        ChatClientMessage::permission_cancelled("req-4")
    );
    assert_eq!(app.chat_state.pending_permission_request_id, None);
    assert_eq!(app.chat_state.pending_permission, None);
    assert_eq!(app.last_action.as_deref(), Some("permission denied"));
}

#[tokio::test]
async fn slash_resume_sends_direct_resume_with_context() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let transport = HttpTransport::new(ServerEndpoint::new(DEFAULT_BASE_URL));
    let mut app = TuiApp::new(&endpoint);
    app.selected_agent = Some("codex".into());
    app.selected_profile = Some("profile-1".into());
    app.selected_workspace = Some("/tmp/project".into());
    app.force_new_session = true;
    app.chat_input = "/resume session-123456789".into();
    let (tx, mut rx) = mpsc::unbounded_channel();

    app.submit_chat_input(&transport, &tx).await;

    assert_eq!(
        rx.try_recv().expect("resume command"),
        ChatClientMessage::ResumeSession {
            agent: Some("codex".into()),
            profile_id: Some("profile-1".into()),
            session_id: "session-123456789".into(),
            session_workspace: Some("/tmp/project".into()),
        }
    );
    assert_eq!(app.selected_session.as_deref(), Some("session-123456789"));
    assert!(!app.force_new_session);
    assert_eq!(
        app.last_action.as_deref(),
        Some("resuming session session-1234")
    );
}

#[tokio::test]
async fn slash_resume_failed_send_keeps_existing_session() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let transport = HttpTransport::new(ServerEndpoint::new(DEFAULT_BASE_URL));
    let mut app = TuiApp::new(&endpoint);
    app.selected_session = Some("old-session".into());
    app.chat_input = "/resume new-session".into();
    let (tx, rx) = mpsc::unbounded_channel();
    drop(rx);

    app.submit_chat_input(&transport, &tx).await;

    assert_eq!(app.selected_session.as_deref(), Some("old-session"));
    assert_eq!(
        app.last_error.as_deref(),
        Some("chat websocket task is not running")
    );
}

#[test]
fn session_ready_updates_chat_context_for_followup_messages() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let mut app = TuiApp::new(&endpoint);
    app.force_new_session = true;
    app.selected_agent = Some("codex".into());
    app.apply_chat_event(ChatEvent::SessionReady {
        session_id: "session-ready-1".into(),
    });
    let (tx, mut rx) = mpsc::unbounded_channel();

    assert_eq!(app.selected_session.as_deref(), Some("session-ready-1"));
    assert_eq!(
        app.chat_state.session_id.as_deref(),
        Some("session-ready-1")
    );
    assert!(!app.force_new_session);

    app.send_chat_message("continue".into(), &tx);

    assert_eq!(
        rx.try_recv().expect("message"),
        ChatClientMessage::Message {
            text: "continue".into(),
            message_id: None,
            agent: Some("codex".into()),
            profile_id: None,
            session_action: Some(ChatSessionAction::Resume),
            session_id: Some("session-ready-1".into()),
            session_workspace: None,
            permission_mode: None,
            attachments: Vec::new(),
        }
    );
}

#[test]
fn chat_context_renders_session_mode() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let mut app = TuiApp::new(&endpoint);
    app.chat_state.session_mode = Some(serde_json::json!({
        "source": "session_mode",
        "currentValue": "acceptEdits",
        "options": [{ "value": "acceptEdits", "name": "Accept edits" }]
    }));

    let backend = TestBackend::new(120, 20);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| render::render(frame, &app))
        .expect("draw");
    let screen = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<Vec<_>>()
        .join("");

    assert!(screen.contains("mode"));
    assert!(screen.contains("Accept edits"));
}

#[test]
fn agent_picker_context_changes_clear_stale_session_context() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let mut app = TuiApp::new(&endpoint);
    app.view = AppView::Agent;
    app.agent_picker.agents = vec![agent("claude")];
    app.agent_picker.profiles = vec![profile("claude-profile")];
    app.agent_picker.workspaces = vec![workspace("/tmp/claude")];
    app.agent_picker.sessions = vec![launch_session("session-1", "claude", "/tmp/session")];
    app.agent_selection.clamp(&app.agent_picker);
    app.selected_agent = Some("codex".into());
    app.selected_profile = Some("codex-profile".into());
    app.selected_workspace = Some("/tmp/codex".into());
    app.selected_session = Some("old-session".into());

    app.enter_current_view();

    assert_eq!(app.selected_agent.as_deref(), Some("claude"));
    assert_eq!(app.selected_profile, None);
    assert_eq!(app.selected_workspace, None);
    assert_eq!(app.selected_session, None);

    app.select_right();
    app.enter_current_view();

    assert_eq!(app.selected_profile.as_deref(), Some("claude-profile"));
    assert_eq!(app.selected_session, None);

    app.select_down();
    app.enter_current_view();

    assert_eq!(app.selected_agent.as_deref(), Some("claude"));
    assert_eq!(app.selected_session.as_deref(), Some("session-1"));
    assert_eq!(app.selected_profile, None);
    assert_eq!(app.selected_workspace.as_deref(), Some("/tmp/session"));

    app.select_left();
    app.enter_current_view();

    assert_eq!(app.selected_workspace.as_deref(), Some("/tmp/claude"));
    assert_eq!(app.selected_session, None);
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
fn system_text_starts_a_separate_response_message() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let mut app = TuiApp::new(&endpoint);

    app.apply_chat_event(ChatEvent::SystemText {
        text: "first answer".into(),
    });
    app.apply_chat_event(ChatEvent::SystemText {
        text: "second answer".into(),
    });

    let responses = app
        .chat_messages
        .iter()
        .filter(|message| message.role == ChatRole::Response)
        .map(|message| message.text.as_str())
        .collect::<Vec<_>>();

    assert_eq!(responses, vec!["first answer", "second answer"]);
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
fn agent_picker_sync_builds_launcher_preference_operations() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let mut app = TuiApp::new(&endpoint);
    app.selected_agent = Some("codex".into());

    app.agent_selection.panel = AgentPanel::Agents;
    let operation = app
        .launcher_preference_operation_for_selection()
        .expect("agent operation")
        .expect("agent writes preference");
    assert_eq!(operation.request().path, "/api/launcher/selected-agent");
    assert_eq!(
        operation.request().body,
        Some(serde_json::json!({ "agentId": "codex" }))
    );

    app.agent_selection.panel = AgentPanel::Profiles;
    app.selected_profile = Some("profile-1".into());
    let operation = app
        .launcher_preference_operation_for_selection()
        .expect("profile operation")
        .expect("profile writes preference");
    assert_eq!(operation.request().path, "/api/launcher/agent-profile");
    assert_eq!(
        operation.request().body,
        Some(serde_json::json!({ "agentId": "codex", "profileId": "profile-1" }))
    );

    app.agent_selection.panel = AgentPanel::Workspaces;
    app.selected_workspace = Some("/tmp/project".into());
    let operation = app
        .launcher_preference_operation_for_selection()
        .expect("workspace operation")
        .expect("workspace writes preference");
    assert_eq!(operation.request().path, "/api/launcher/agent-workspace");
    assert_eq!(
        operation.request().body,
        Some(serde_json::json!({ "agentId": "codex", "workspace": "/tmp/project" }))
    );

    app.agent_selection.panel = AgentPanel::Sessions;
    assert!(app
        .launcher_preference_operation_for_selection()
        .expect("session sync decision")
        .is_none());
}

#[test]
fn agent_picker_sync_requires_agent_for_profile_or_workspace_preferences() {
    let endpoint = ServerEndpoint::new(DEFAULT_BASE_URL);
    let mut app = TuiApp::new(&endpoint);
    app.agent_selection.panel = AgentPanel::Profiles;
    app.selected_profile = Some("profile-1".into());

    let error = match app.launcher_preference_operation_for_selection() {
        Err(error) => error,
        Ok(_) => panic!("expected missing agent error"),
    };
    assert_eq!(error, "select an agent before choosing a profile");

    app.agent_selection.panel = AgentPanel::Workspaces;
    app.selected_profile = None;
    app.selected_workspace = Some("/tmp/project".into());
    let error = match app.launcher_preference_operation_for_selection() {
        Err(error) => error,
        Ok(_) => panic!("expected missing agent error"),
    };
    assert_eq!(error, "select an agent before choosing a workspace");
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
