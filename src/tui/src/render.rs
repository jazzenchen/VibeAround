use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;
use va_client::runtime::{AgentRuntime, ChannelRuntime, TunnelRuntime};
use va_client::sessions::SessionListItem;

use crate::app::{AppView, TuiApp};
use crate::chat::{chat_message_lines_for_messages, input_box_height, visible_chat_lines};
use crate::selection::{AgentPanel, RuntimePanel};
use crate::theme::{muted_style, BRAND, WARN};

mod brand;
mod chrome;
mod rows;

use brand::{brand_header, brand_mode};
use chrome::{command_bar, context_strip};
use rows::{
    agent_info_row, agent_row, channel_row, profile_row, session_row, tunnel_row, workspace_row,
};

#[cfg(test)]
pub(crate) use chrome::view_hint;

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
