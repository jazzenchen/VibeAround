use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Padding, Paragraph, Wrap};
use ratatui::Frame;
use va_client::runtime::{AgentRuntime, ChannelRuntime, TunnelRuntime};
use va_client::sessions::SessionListItem;

use crate::app::{AppView, TuiApp};
use crate::chat::{
    chat_message_lines_for_messages, input_box_height, input_cursor_offset, input_visible_lines,
    visible_chat_lines, SlashCommand,
};
use crate::selection::{AgentPanel, RuntimePanel};
use crate::theme::{
    accent_style, muted_style, ACTION, BRAND, INPUT_ACCENT, INPUT_BG, SEMANTIC_BORDER,
};

mod brand;
mod chrome;
mod rows;

use brand::{mark_lines, wordmark_lines, MARK_WIDTH, VERSION};
use chrome::{
    chat_context_spans, command_bar, context_pairs, context_strip, divider_line, label_value_spans,
};
use rows::{
    agent_info_row, agent_row, channel_row, launch_session_row, profile_row, session_row,
    tunnel_row, workspace_row,
};

const INPUT_HORIZONTAL_PADDING: u16 = 2;
const MAX_INPUT_ROWS: u16 = 4;
const CONTENT_INSET: u16 = 2;
const INPUT_BORDER: u16 = 1;
/// Rows in the VA mark; the header info is sized to align with it.
const WORKING_HEADER_HEIGHT: u16 = 4;

#[cfg(test)]
pub(crate) use chrome::view_hint;

pub(crate) fn render(frame: &mut Frame<'_>, app: &TuiApp) {
    let area = frame.area();
    if app.is_welcome() {
        render_welcome(frame, app, area);
        return;
    }
    if app.view == AppView::Chat {
        render_working_chat(frame, app, area);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(4),
            Constraint::Length(2),
        ])
        .split(area);
    let context_area = content_rect(chunks[0]);
    let body_area = content_rect(chunks[1]);
    let footer_area = content_rect(chunks[2]);

    frame.render_widget(context_strip(app, context_area.width), context_area);
    match app.view {
        AppView::Status => render_status_view(frame, app, body_area),
        AppView::StatusDetail => render_status_detail_view(frame, app, body_area),
        AppView::Agent => render_agent_view(frame, app, body_area),
        AppView::Chat => {}
    }
    frame.render_widget(command_bar(app, footer_area.width), footer_area);
}

/// The working chat: a Claude-style header (brand mark + key info), the
/// conversation, and the input — the layout once a session is underway.
fn render_working_chat(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(WORKING_HEADER_HEIGHT),
            Constraint::Length(1),
            Constraint::Min(4),
            Constraint::Length(2),
        ])
        .split(area);
    let header_area = content_rect(chunks[0]);
    let rule_area = content_rect(chunks[1]);
    let body_area = content_rect(chunks[2]);
    let footer_area = content_rect(chunks[3]);

    render_working_header(frame, app, header_area);
    frame.render_widget(
        Paragraph::new(divider_line(rule_area.width)),
        rule_area,
    );
    render_chat_view(frame, app, body_area);
    frame.render_widget(command_bar(app, footer_area.width), footer_area);
}

/// Brand mark on the left, a few lines of key info on the right — keeps the
/// identity present without a full-height wordmark.
fn render_working_header(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    frame.render_widget(
        Paragraph::new(mark_lines()),
        Rect {
            x: area.x,
            y: area.y,
            width: MARK_WIDTH.min(area.width),
            height: area.height,
        },
    );

    let info_x = area.x.saturating_add(MARK_WIDTH).saturating_add(3);
    let info_width = area.width.saturating_sub(MARK_WIDTH.saturating_add(3));
    if info_width == 0 {
        return;
    }

    // Four info lines, aligned row-for-row with the four-row mark:
    //   brand + version / agent · profile / workspace / session (· mode).
    let pairs = context_pairs(app);
    let joined = |slice: &[(&'static str, String)]| {
        let mut spans = Vec::new();
        for (index, (label, value)) in slice.iter().enumerate() {
            if index > 0 {
                spans.push(Span::styled("   ·   ", muted_style()));
            }
            spans.extend(label_value_spans(label, value));
        }
        Line::from(spans)
    };
    let info = vec![
        Line::from(vec![
            Span::styled("VibeAround", accent_style()),
            Span::styled(format!("  {VERSION}"), muted_style()),
        ]),
        joined(&pairs[0..2.min(pairs.len())]),
        joined(&pairs[2..3.min(pairs.len())]),
        joined(&pairs[3..]),
    ];

    // Center the info lines against the mark.
    let info_height = u16::try_from(info.len()).unwrap_or(0);
    let info_y = area.y + area.height.saturating_sub(info_height) / 2;
    frame.render_widget(
        Paragraph::new(info),
        Rect {
            x: info_x,
            y: info_y,
            width: info_width,
            height: info_height,
        },
    );
}

/// The centered launch screen shown before a conversation begins: the brand
/// wordmark over a single clean input, with the active context and key hints
/// beneath. The big wordmark lives here and nowhere else, so working views
/// stay compact.
fn render_welcome(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let area = content_rect(area);
    let wordmark = wordmark_lines(area.width);
    let wordmark_height = u16::try_from(wordmark.len()).unwrap_or(0);

    let box_width = area.width.clamp(24, 76);
    let input_padding = input_horizontal_padding(box_width);
    let input_content_width =
        usize::from(box_width.saturating_sub(input_padding.saturating_mul(2))).max(1);
    let input_height = input_box_height(&app.chat_input, input_content_width, MAX_INPUT_ROWS);

    // wordmark + gap + input + gap + context + gap + hints
    let block_height = wordmark_height
        .saturating_add(input_height)
        .saturating_add(5);
    let mut y = area.y + area.height.saturating_sub(block_height) / 2;
    let input_x = area.x + area.width.saturating_sub(box_width) / 2;

    frame.render_widget(
        Paragraph::new(wordmark).alignment(Alignment::Center),
        Rect {
            x: area.x,
            y,
            width: area.width,
            height: wordmark_height,
        },
    );
    y = y.saturating_add(wordmark_height).saturating_add(1);

    let input_rect = Rect {
        x: input_x,
        y,
        width: box_width,
        height: input_height,
    };
    render_input_bar(frame, app, input_rect);
    render_slash_popup(frame, app, input_rect);
    y = y.saturating_add(input_height).saturating_add(1);

    frame.render_widget(
        Paragraph::new(Line::from(chat_context_spans(app))).alignment(Alignment::Center),
        Rect {
            x: area.x,
            y,
            width: area.width,
            height: 1,
        },
    );
    y = y.saturating_add(2);

    frame.render_widget(
        Paragraph::new(welcome_tip_line()).alignment(Alignment::Center),
        Rect {
            x: area.x,
            y,
            width: area.width,
            height: 1,
        },
    );

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(VERSION, muted_style()))).alignment(Alignment::Right),
        Rect {
            x: area.x,
            y: area.y + area.height.saturating_sub(1),
            width: area.width,
            height: 1,
        },
    );
}

/// A single descriptive hint, styled like a tip rather than a key legend.
fn welcome_tip_line() -> Line<'static> {
    Line::from(vec![
        Span::styled("● ", accent_style()),
        Span::styled("Tip", accent_style()),
        Span::styled("  type ", muted_style()),
        Span::styled("/", accent_style()),
        Span::styled(" for commands, or just write and press ", muted_style()),
        Span::styled("Enter", accent_style()),
    ])
}

/// Width available for text inside the input bar, after the accent rail and
/// horizontal padding.
fn input_inner_width(area_width: u16) -> usize {
    let padding = input_horizontal_padding(area_width);
    usize::from(
        area_width
            .saturating_sub(INPUT_BORDER)
            .saturating_sub(padding.saturating_mul(2)),
    )
    .max(1)
}

/// Render the input strip (brand accent rail + subtle tint + prompt) into
/// `area` and place the text cursor inside it. Shared by the welcome and
/// working chat layouts.
fn render_input_bar(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let input_padding = input_horizontal_padding(area.width);
    let input_content_width = input_inner_width(area.width);
    frame.render_widget(
        Paragraph::new(chat_input_lines(
            &app.chat_input,
            app.chat_cursor,
            input_content_width,
            MAX_INPUT_ROWS,
        ))
        .wrap(Wrap { trim: false })
        .block(input_block(input_padding))
        .style(Style::default().bg(INPUT_BG)),
        area,
    );
    let (cursor_x, cursor_y) = input_cursor_offset(
        &app.chat_input,
        app.chat_cursor,
        input_content_width,
        MAX_INPUT_ROWS,
    );
    frame.set_cursor_position((
        area.x
            .saturating_add(INPUT_BORDER)
            .saturating_add(input_padding)
            .saturating_add(cursor_x),
        area.y.saturating_add(1).saturating_add(cursor_y),
    ));
}

const SLASH_POPUP_MAX_ROWS: usize = 8;

/// Autocomplete menu for slash commands, anchored just above `input_area` and
/// growing upward — a bottom-up popup over the conversation.
fn render_slash_popup(frame: &mut Frame<'_>, app: &TuiApp, input_area: Rect) {
    let Some(matches) = app.slash_matches() else {
        return;
    };
    let selected = app.slash_selection.min(matches.len().saturating_sub(1));

    let mut lines = vec![Line::from(Span::styled("commands", muted_style()))];
    for (index, command) in matches.iter().take(SLASH_POPUP_MAX_ROWS).enumerate() {
        lines.push(slash_popup_row(command, index == selected));
    }

    let height = u16::try_from(lines.len()).unwrap_or(0);
    let top = input_area.y.saturating_sub(height);
    let rect = Rect {
        x: input_area.x,
        y: top,
        width: input_area.width,
        height: input_area.y.saturating_sub(top),
    };
    if rect.height == 0 {
        return;
    }

    let block = Block::default()
        .borders(Borders::LEFT)
        .border_set(INPUT_ACCENT)
        .border_style(Style::default().fg(BRAND))
        .padding(Padding::new(1, 1, 0, 0))
        .style(Style::default().bg(INPUT_BG));
    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).block(block), rect);
}

fn slash_popup_row(command: &SlashCommand, selected: bool) -> Line<'static> {
    let name_style = if selected {
        accent_style()
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    };
    Line::from(vec![
        Span::styled(if selected { "› " } else { "  " }, accent_style()),
        Span::styled(format!("{:<9}", command.name), name_style),
        Span::styled(format!(" {}", command.summary), muted_style()),
    ])
}

fn content_rect(area: Rect) -> Rect {
    let inset = if area.width > CONTENT_INSET.saturating_mul(2) {
        CONTENT_INSET
    } else {
        0
    };
    Rect {
        x: area.x.saturating_add(inset),
        y: area.y,
        width: area.width.saturating_sub(inset.saturating_mul(2)),
        height: area.height,
    }
}

fn render_chat_view(frame: &mut Frame<'_>, app: &TuiApp, area: ratatui::layout::Rect) {
    let input_height =
        input_box_height(&app.chat_input, input_inner_width(area.width), MAX_INPUT_ROWS);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .spacing(1)
        .constraints([Constraint::Min(4), Constraint::Length(input_height)])
        .split(area);
    let visible_rows = usize::from(chunks[0].height);
    let content_width = usize::from(chunks[0].width.saturating_sub(1)).max(1);
    // Conversation flows from the top; once it overflows, the latest lines stay
    // pinned to the bottom via the scroll offset.
    let message_lines = visible_chat_lines(
        chat_message_lines_for_messages(&app.chat_messages, content_width),
        visible_rows,
        app.chat_scroll,
    );
    frame.render_widget(
        List::new(
            message_lines
                .into_iter()
                .map(ListItem::new)
                .collect::<Vec<_>>(),
        ),
        chunks[0],
    );
    render_input_bar(frame, app, chunks[1]);
    render_slash_popup(frame, app, chunks[1]);
}

fn render_status_view(frame: &mut Frame<'_>, app: &TuiApp, area: ratatui::layout::Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .spacing(3)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let top_rows = app.snapshot.channels.len().max(app.snapshot.agents.len());
    let bottom_rows = app.snapshot.tunnels.len().max(app.snapshot.sessions.len());
    let left = panel_column(columns[0], top_rows, bottom_rows);
    let right = panel_column(columns[1], top_rows, bottom_rows);

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
        .spacing(3)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let top_rows = app.agent_picker.agents.len().max(app.agent_picker.profiles.len());
    let bottom_rows = app
        .agent_picker
        .workspaces
        .len()
        .max(app.agent_picker.sessions.len());
    let left = panel_column(columns[0], top_rows, bottom_rows);
    let right = panel_column(columns[1], top_rows, bottom_rows);

    frame.render_widget(
        picker_list(
            "agents",
            app.agent_picker
                .agents
                .iter()
                .map(|agent| {
                    selected_context_row(
                        agent_info_row(agent),
                        app.effective_agent() == Some(agent.id.as_str()),
                    )
                })
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
                .map(|workspace| {
                    selected_context_row(
                        workspace_row(workspace),
                        app.effective_workspace() == Some(workspace.path.as_str()),
                    )
                })
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
                .map(|profile| {
                    selected_context_row(
                        profile_row(profile),
                        app.effective_profile() == Some(profile.id.as_str()),
                    )
                })
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
                .map(|session| {
                    selected_context_row(
                        launch_session_row(session),
                        app.effective_session() == Some(session.session_id.as_str()),
                    )
                })
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

/// Split a column into a top panel and a bottom panel sized to their
/// content, pooling any leftover height at the bottom instead of stretching
/// every panel to fill half the column.
fn panel_column(area: Rect, top_rows: usize, bottom_rows: usize) -> std::rc::Rc<[Rect]> {
    Layout::default()
        .direction(Direction::Vertical)
        .spacing(1)
        .constraints([
            Constraint::Length(panel_height(top_rows)),
            Constraint::Length(panel_height(bottom_rows)),
            Constraint::Min(0),
        ])
        .split(area)
}

/// Heading + blank spacer + at least one body row.
fn panel_height(rows: usize) -> u16 {
    u16::try_from(2 + rows.max(1)).unwrap_or(u16::MAX)
}

fn selected_context_row(mut row: Vec<Span<'static>>, selected: bool) -> Vec<Span<'static>> {
    let mut spans = Vec::with_capacity(row.len() + 1);
    spans.push(Span::styled(
        if selected { "● " } else { "  " },
        if selected { accent_style() } else { muted_style() },
    ));
    spans.append(&mut row);
    spans
}

fn selectable_list(
    title: &'static str,
    rows: Vec<Vec<Span<'static>>>,
    selected: Option<usize>,
    active: bool,
) -> List<'static> {
    let mut items = vec![
        ListItem::new(section_heading(title, rows.len(), active)),
        ListItem::new(Line::raw("")),
    ];
    if rows.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            "no entries",
            muted_style(),
        ))));
    } else {
        items.extend(rows.into_iter().enumerate().map(|(index, row)| {
            let is_cursor = active && Some(index) == selected;
            let marker = if is_cursor { "› " } else { "  " };
            let mut spans = vec![Span::styled(marker, accent_style())];
            spans.extend(row);
            let item = ListItem::new(Line::from(spans));
            if is_cursor {
                item.style(Style::default().add_modifier(Modifier::BOLD))
            } else {
                item
            }
        }))
    };
    List::new(items).block(panel_block(active))
}

fn section_heading(title: &str, count: usize, active: bool) -> Line<'static> {
    let title_style = if active {
        accent_style()
    } else {
        muted_style().add_modifier(Modifier::BOLD)
    };
    Line::from(vec![
        Span::styled(title.trim().to_string(), title_style),
        Span::styled(format!("  {count}"), muted_style()),
    ])
}

/// The active panel is marked by a brand-colored left rail; everything else
/// is indented to match so the columns stay aligned without boxes.
fn panel_block(active: bool) -> Block<'static> {
    if active {
        Block::default()
            .borders(Borders::LEFT)
            .border_set(SEMANTIC_BORDER)
            .border_style(Style::default().fg(ACTION))
            .padding(Padding::new(1, 0, 0, 0))
    } else {
        Block::default().padding(Padding::new(2, 0, 0, 0))
    }
}

fn focus_block(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::LEFT)
        .border_set(SEMANTIC_BORDER)
        .border_style(Style::default().fg(ACTION))
        .title_style(accent_style())
        .title(format!(" {title} "))
        .padding(Padding::new(1, 0, 0, 0))
}

/// A subtly tinted strip with a brand accent rail down the left — a Claude
/// Code-style input zone rather than a bordered box.
fn input_block(horizontal_padding: u16) -> Block<'static> {
    Block::default()
        .borders(Borders::LEFT)
        .border_set(INPUT_ACCENT)
        .border_style(Style::default().fg(BRAND))
        .padding(Padding::new(horizontal_padding, horizontal_padding, 1, 1))
        .style(Style::default().bg(INPUT_BG))
}

fn input_horizontal_padding(width: u16) -> u16 {
    if width >= INPUT_HORIZONTAL_PADDING.saturating_mul(2) + 1 {
        INPUT_HORIZONTAL_PADDING
    } else {
        0
    }
}

fn chat_input_lines(
    input: &str,
    cursor: usize,
    content_width: usize,
    max_body_rows: u16,
) -> Vec<Line<'static>> {
    if input.is_empty() {
        return vec![Line::from(vec![
            Span::styled(
                "› ",
                Style::default().fg(ACTION).add_modifier(Modifier::BOLD),
            ),
            Span::styled("type a message, or /command", muted_style()),
        ])];
    }

    input_visible_lines(input, cursor, content_width, max_body_rows)
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            let marker = if index == 0 { "› " } else { "  " };
            Line::from(vec![
                Span::styled(
                    marker,
                    Style::default().fg(ACTION).add_modifier(Modifier::BOLD),
                ),
                Span::raw(line),
            ])
        })
        .collect()
}
