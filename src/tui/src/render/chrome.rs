use ratatui::layout::Alignment;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{AppView, TuiApp};
use crate::chat::session_mode_display_label;
use crate::theme::{muted_style, BRAND, ERROR, OK, SEMANTIC_BORDER, WARN};

pub(super) fn context_strip(app: &TuiApp) -> Paragraph<'static> {
    let spans = match app.view {
        AppView::Chat => chat_context_spans(app),
        AppView::Status | AppView::StatusDetail => status_context_spans(app),
        AppView::Agent => agent_context_spans(app),
    };
    Paragraph::new(Line::from(spans))
        .alignment(Alignment::Center)
        .block(chrome_separator())
}

fn chat_context_spans(app: &TuiApp) -> Vec<Span<'static>> {
    let session_label = app
        .effective_session()
        .or(app.chat_state.session_id.as_deref())
        .map(short_id)
        .unwrap_or_else(|| "new".to_string());
    let agent_label = app
        .effective_agent()
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
        app.effective_profile().unwrap_or("global"),
    ));
    spans.push(Span::raw("  "));
    spans.extend(label_value_spans(
        "workspace",
        app.effective_workspace().unwrap_or("global"),
    ));
    spans.push(Span::raw("  "));
    spans.extend(label_value_spans("session", &session_label));
    if let Some(mode) = session_mode_display_label(app.chat_state.session_mode.as_ref()) {
        spans.push(Span::raw("  "));
        spans.extend(label_value_spans("mode", &mode));
    }
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
    spans.extend(metric_spans("channels", app.snapshot.channels.len()));
    spans.push(Span::raw("  "));
    spans.extend(metric_spans("tunnels", app.snapshot.tunnels.len()));
    spans.push(Span::raw("  "));
    spans.extend(metric_spans("agents", app.snapshot.agents.len()));
    spans.push(Span::raw("  "));
    spans.extend(metric_spans("sessions", app.snapshot.sessions.len()));
    spans
}

fn agent_context_spans(app: &TuiApp) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(
        "agent context",
        Style::default().fg(BRAND).add_modifier(Modifier::BOLD),
    )];
    spans.push(separator());
    spans.extend(label_value_spans(
        "agent",
        app.effective_agent().unwrap_or("global"),
    ));
    spans.push(Span::raw("  "));
    spans.extend(label_value_spans(
        "profile",
        app.effective_profile().unwrap_or("global"),
    ));
    spans.push(Span::raw("  "));
    spans.extend(label_value_spans(
        "workspace",
        app.effective_workspace().unwrap_or("global"),
    ));
    spans.push(Span::raw("  "));
    spans.extend(label_value_spans(
        "session",
        &app.effective_session()
            .map(short_id)
            .unwrap_or_else(|| "new".into()),
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

fn metric_spans(label: &'static str, value: usize) -> Vec<Span<'static>> {
    vec![
        Span::styled(label, muted_style()),
        Span::raw(" "),
        Span::styled(
            value.to_string(),
            Style::default().fg(BRAND).add_modifier(Modifier::BOLD),
        ),
    ]
}

fn short_id(value: &str) -> String {
    value.chars().take(12).collect()
}

pub(super) fn command_bar(app: &TuiApp) -> Paragraph<'static> {
    let (status, status_style) = if app.exit_confirmation_pending() {
        (
            "press Ctrl+C again to quit".to_string(),
            Style::default().fg(WARN),
        )
    } else if let Some(error) = &app.last_error {
        (format!("error: {error}"), Style::default().fg(ERROR))
    } else if matches!(app.view, AppView::Chat)
        && (app.chat_state.pending_permission_request_id.is_some()
            || app.chat_state.turn_active
            || app.chat_scroll > 0)
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
        .block(chrome_separator())
}

pub(crate) fn view_hint(app: &TuiApp) -> String {
    match app.view {
        AppView::Chat => {
            if app.chat_state.pending_permission_request_id.is_some() {
                "permission pending: /allow [number|option-id] or /deny".to_string()
            } else if app.chat_state.turn_active {
                app.work_status
                    .clone()
                    .unwrap_or_else(|| "agent is working; /stop to interrupt".to_string())
            } else if app.chat_scroll > 0 {
                format!(
                    "scrollback {} lines; Down/PageDown moves toward latest",
                    app.chat_scroll
                )
            } else if app.force_new_session {
                "next message starts a new session".to_string()
            } else {
                "type a message or slash command".to_string()
            }
        }
        AppView::Status => app
            .last_refresh
            .map(|instant| {
                format!(
                    "runtime stream updated {}s ago",
                    instant.elapsed().as_secs()
                )
            })
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
            key_span("/new"),
            Span::raw("  "),
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
        Style::default().fg(BRAND).add_modifier(Modifier::BOLD),
    )
}

fn chrome_separator() -> Block<'static> {
    Block::default()
        .borders(Borders::TOP)
        .border_set(SEMANTIC_BORDER)
        .border_style(muted_style())
}
