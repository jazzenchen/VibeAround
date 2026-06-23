use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;
use va_client::runtime::{AgentRuntime, ChannelRuntime, TunnelRuntime};
use va_client::sessions::SessionListItem;

use crate::app::{AppView, TuiApp};
use crate::chat::{chat_message_lines_for_messages, input_box_height, visible_chat_lines};
use crate::selection::{AgentPanel, RuntimePanel};
use crate::theme::{muted_style, BRAND, ERROR, OK, WARN};

mod brand;
mod rows;

use brand::{brand_header, brand_mode};
use rows::{
    agent_info_row, agent_row, channel_row, profile_row, session_row, tunnel_row, workspace_row,
};

pub(crate) fn render(frame: &mut Frame<'_>, app: &TuiApp) {
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

fn context_strip(app: &TuiApp) -> Paragraph<'static> {
    let spans = match app.view {
        AppView::Chat => chat_context_spans(app),
        AppView::Status | AppView::StatusDetail => status_context_spans(app),
        AppView::Agent => agent_context_spans(app),
    };
    Paragraph::new(Line::from(spans)).alignment(Alignment::Center)
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

fn short_id(value: &str) -> String {
    value.chars().take(12).collect()
}

fn render_chat_view(frame: &mut Frame<'_>, app: &TuiApp, area: ratatui::layout::Rect) {
    let input_content_width = usize::from(area.width.saturating_sub(4)).max(1);
    let input_height = input_box_height(&app.chat_input, input_content_width, 4);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(input_height)])
        .split(area);
    let visible_rows = usize::from(chunks[0].height.saturating_sub(2));
    let content_width = usize::from(chunks[0].width.saturating_sub(2)).max(1);
    let message_lines = if app.chat_messages.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "Type /help for commands.",
            muted_style(),
        )))]
    } else {
        visible_chat_lines(
            chat_message_lines_for_messages(&app.chat_messages, content_width),
            visible_rows,
            app.chat_scroll,
        )
        .into_iter()
        .map(ListItem::new)
        .collect()
    };
    let mut messages = vec![
        ListItem::new(section_heading("chat", false)),
        ListItem::new(Line::raw("")),
    ];
    messages.extend(message_lines);
    frame.render_widget(List::new(messages), chunks[0]);
    frame.render_widget(
        Paragraph::new(vec![Line::from(vec![
            Span::styled("› ", Style::default().fg(BRAND)),
            Span::raw(app.chat_input.clone()),
        ])])
        .wrap(Wrap { trim: false })
        .block(focus_block("message")),
        chunks[1],
    );
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
    frame.render_widget(Paragraph::new(lines).block(focus_block(title.trim())), area);
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
    let mut items = Vec::new();
    if rows.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            "  no runtime entries",
            muted_style(),
        ))));
    } else {
        items.extend(rows.into_iter().enumerate().map(|(index, row)| {
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
        }))
    };
    let block = if active {
        focus_block(title)
    } else {
        quiet_block(title)
    };
    List::new(items).block(block)
}

fn section_heading(title: &str, active: bool) -> Line<'static> {
    let marker = if active { "› " } else { "  " };
    let style = if active {
        Style::default().fg(BRAND).add_modifier(Modifier::BOLD)
    } else {
        muted_style().add_modifier(Modifier::BOLD)
    };
    Line::from(vec![
        Span::styled(marker, style),
        Span::styled(title.trim().to_string(), style),
    ])
}

fn focus_block(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BRAND))
        .title_style(Style::default().fg(BRAND).add_modifier(Modifier::BOLD))
        .title(format!(" {title} "))
}

fn quiet_block(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::TOP)
        .border_style(muted_style())
        .title_style(muted_style().add_modifier(Modifier::BOLD))
        .title(format!(" {title} "))
}

fn command_bar(app: &TuiApp) -> Paragraph<'static> {
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
    Paragraph::new(Line::from(spans)).alignment(Alignment::Center)
}

pub(crate) fn view_hint(app: &TuiApp) -> String {
    match app.view {
        AppView::Chat => {
            if app.chat_state.pending_permission_request_id.is_some() {
                "permission pending: /allow <option-id> or /deny".to_string()
            } else if app.chat_state.turn_active {
                app.work_status
                    .clone()
                    .unwrap_or_else(|| "agent is working; /stop to interrupt".to_string())
            } else if app.chat_scroll > 0 {
                format!(
                    "scrollback {} lines; Down/PageDown returns to latest",
                    app.chat_scroll
                )
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
