use super::*;
use crate::config::DEFAULT_BASE_URL;
use crate::render;
use crate::selection::RuntimePanel;
use tokio::sync::mpsc;
use va_client::events::{ChatClientMessage, ChatEvent, ChatSessionAction};
use va_client::runtime::{AgentInfo, ChannelRuntime, ChannelStatus, TunnelRuntime, TunnelStatus};

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
