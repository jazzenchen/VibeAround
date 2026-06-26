use ratatui::layout::Alignment;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::TuiApp;
use crate::chat::session_mode_display_label;
use crate::theme::{muted_style, ACTION, ERROR, WARN};

/// The active chat context as label/value pairs (agent, profile, workspace,
/// session, and mode when set). Rendered as one line on the welcome screen
/// and split across the working header.
pub(super) fn context_pairs(app: &TuiApp) -> Vec<(&'static str, String)> {
    let session_label = app
        .effective_session()
        .or(app.chat_state.session_id.as_deref())
        .map(str::to_string)
        .unwrap_or_else(|| "new".to_string());
    let agent_label = app
        .effective_agent()
        .or(app.chat_state.default_agent.as_deref())
        .unwrap_or("global")
        .to_string();
    let mut pairs = vec![
        ("agent", agent_label),
        (
            "profile",
            app.effective_profile().unwrap_or("direct").to_string(),
        ),
        (
            "workspace",
            app.effective_workspace().unwrap_or("global").to_string(),
        ),
        ("session", session_label),
    ];
    if let Some(mode) = session_mode_display_label(app.chat_state.session_mode.as_ref()) {
        pairs.push(("mode", mode));
    }
    pairs
}

pub(super) fn label_value_spans(label: &'static str, value: &str) -> Vec<Span<'static>> {
    vec![
        Span::styled(label, muted_style()),
        Span::raw(" "),
        Span::styled(
            value.to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]
}

pub(super) fn command_bar(app: &TuiApp, content_width: u16) -> Paragraph<'static> {
    let (status, status_style) = if app.exit_confirmation_pending() {
        (
            "press Ctrl+C again to quit".to_string(),
            Style::default().fg(WARN),
        )
    } else if let Some(error) = &app.last_error {
        (format!("error: {error}"), Style::default().fg(ERROR))
    } else if !app.popup_is_open()
        && (app.chat_state.pending_permission_request_id.is_some() || app.chat_state.turn_active)
    {
        (view_hint(app), muted_style())
    } else if let Some(action) = &app.last_action {
        (format!("last: {action}"), muted_style())
    } else {
        (view_hint(app), muted_style())
    };
    let mut spans = vec![
        Span::styled(status, status_style),
        Span::styled("  |  ", muted_style()),
    ];
    spans.extend(view_command_spans());
    spans.extend([
        Span::styled("  |  ", muted_style()),
        key_span("Ctrl+C"),
        Span::styled(" ×2 quit", muted_style()),
    ]);
    Paragraph::new(vec![divider_line(content_width), Line::from(spans)]).alignment(Alignment::Left)
}

pub(crate) fn view_hint(app: &TuiApp) -> String {
    if app.popup_is_open() {
        "↑↓ move · Enter open/select · Esc back".to_string()
    } else if app.slash_popup_open() {
        "↑↓ select · Tab complete · Enter run".to_string()
    } else if app.chat_state.pending_permission_request_id.is_some() {
        "permission pending: /allow [number|option-id] or /deny".to_string()
    } else if app.chat_state.turn_active {
        app.work_status
            .clone()
            .unwrap_or_else(|| "agent is working; /stop to interrupt".to_string())
    } else if app.force_new_session {
        "next message starts a new session".to_string()
    } else {
        "type a message or slash command".to_string()
    }
}

fn view_command_spans() -> Vec<Span<'static>> {
    vec![
        key_span("Enter"),
        Span::raw(" send  "),
        key_span("/new"),
        Span::raw("  "),
        key_span("/status"),
        Span::raw("  "),
        key_span("/agent"),
        Span::raw("  "),
        key_span("/help"),
    ]
}

fn key_span(value: &'static str) -> Span<'static> {
    Span::styled(
        value,
        Style::default().fg(ACTION).add_modifier(Modifier::BOLD),
    )
}

pub(super) fn divider_line(width: u16) -> Line<'static> {
    Line::from(Span::styled("─".repeat(usize::from(width)), muted_style()))
}
