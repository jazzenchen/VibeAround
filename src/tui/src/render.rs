use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, List, ListItem, Padding, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::TuiApp;
use crate::chat::{
    chat_message_lines_for_messages, input_box_height, input_cursor_offset, input_visible_lines,
    visible_chat_lines, SlashCommand,
};
use crate::detail::{agent_detail, channel_detail, session_detail, tunnel_detail};
use crate::popup::{Popup, PopupLevel};
use crate::theme::{
    accent_style, muted_style, ACTION, BRAND, ERROR, INPUT_ACCENT, INPUT_BG, OK, WARN,
};

mod brand;
mod chrome;
mod rows;

use brand::{mark_lines, wordmark_lines, MARK_WIDTH, VERSION};
use chrome::{
    command_bar, context_pairs, divider_line, label_value_spans,
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
    // The welcome screen only shows before a conversation starts and while no
    // popup is open; otherwise the working chat (and any overlay) is rendered.
    if app.popup.is_none() && app.is_welcome() {
        render_welcome(frame, app, area);
    } else {
        render_working_chat(frame, app, area);
    }
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
    let mut info = vec![Line::from(vec![
        Span::styled("VibeAround", accent_style()),
        Span::styled(format!("  {VERSION}"), muted_style()),
    ])];
    info.extend(context_grouped_lines(app));

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

/// Active context grouped into lines — `agent · profile`, then `workspace`
/// and `session` (with mode) each on their own so long paths and ids aren't
/// crowded. Used by the working header.
fn context_grouped_lines(app: &TuiApp) -> Vec<Line<'static>> {
    let pairs = context_pairs(app);
    vec![
        context_line(&pairs[0..2.min(pairs.len())]),
        context_line(&pairs[2..3.min(pairs.len())]),
        context_line(&pairs[3..]),
    ]
}

fn context_line(slice: &[(&'static str, String)]) -> Line<'static> {
    context_line_with_separator(slice, "   ·   ")
}

fn context_line_with_separator(
    slice: &[(&'static str, String)],
    separator: &'static str,
) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, (label, value)) in slice.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(separator, muted_style()));
        }
        spans.extend(label_value_spans(label, value));
    }
    Line::from(spans)
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

    let context = welcome_context_lines(app);
    let context_height = u16::try_from(context.len()).unwrap_or(0);

    // wordmark + gap + input + gap + context + gap + tip
    let block_height = wordmark_height
        .saturating_add(input_height)
        .saturating_add(context_height)
        .saturating_add(4);
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
        Paragraph::new(context).alignment(Alignment::Center),
        Rect {
            x: area.x,
            y,
            width: area.width,
            height: context_height,
        },
    );
    y = y.saturating_add(context_height).saturating_add(1);

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

fn welcome_context_lines(app: &TuiApp) -> Vec<Line<'static>> {
    let pairs = context_pairs(app);
    vec![context_line_with_separator(
        &pairs[0..3.min(pairs.len())],
        "  ·  ",
    )]
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
const COMMAND_POPUP_MAX_ROWS: usize = 10;

/// Draw a floating menu that rises from just above `input_area` and covers the
/// conversation. A one-row gap and a rounded brand border set it apart from the
/// input bar — it reads as an overlay, not an extension of the input.
fn render_bottom_popup(frame: &mut Frame<'_>, input_area: Rect, lines: Vec<Line<'static>>) {
    const GAP: u16 = 1;
    // content rows + top and bottom border
    let height = u16::try_from(lines.len()).unwrap_or(0).saturating_add(2);
    let bottom = input_area.y.saturating_sub(GAP);
    let top = bottom.saturating_sub(height);
    let rect = Rect {
        x: input_area.x,
        y: top,
        width: input_area.width,
        height: bottom.saturating_sub(top),
    };
    if rect.height < 2 {
        return;
    }
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BRAND))
        .padding(Padding::horizontal(1));
    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).block(block), rect);
}

/// Autocomplete menu for slash commands.
fn render_slash_popup(frame: &mut Frame<'_>, app: &TuiApp, input_area: Rect) {
    let Some(matches) = app.slash_matches() else {
        return;
    };
    let selected = app.slash_selection.min(matches.len().saturating_sub(1));
    let mut lines = vec![Line::from(Span::styled("commands", muted_style()))];
    for (index, command) in matches.iter().take(SLASH_POPUP_MAX_ROWS).enumerate() {
        lines.push(slash_popup_row(command, index == selected));
    }
    render_bottom_popup(frame, input_area, lines);
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

/// Content lines for the `/status` and `/agent` drill-down popup.
fn command_popup_lines(app: &TuiApp) -> Vec<Line<'static>> {
    let Some(popup) = &app.popup else {
        return Vec::new();
    };
    use crate::popup::PopupKind;
    let mut lines = vec![popup_breadcrumb(popup)];
    match popup.level {
        PopupLevel::Categories => {
            for (index, label) in popup.kind.categories().iter().enumerate() {
                // The agent menu doubles as a config summary (each category
                // shows its selection); status shows a count plus health.
                let trailing = match popup.kind {
                    PopupKind::Agent => {
                        vec![Span::styled(app.agent_category_value(index), accent_style())]
                    }
                    PopupKind::Status => status_category_trailing(app, index),
                };
                lines.push(popup_category_row(label, trailing, index == popup.cursor));
            }
        }
        PopupLevel::Items { category } => {
            let rows = popup_item_rows(app, popup, category);
            if rows.is_empty() {
                lines.push(Line::from(Span::styled("  no entries", muted_style())));
            } else {
                for index in popup_window(rows.len(), popup.cursor, COMMAND_POPUP_MAX_ROWS) {
                    // The `●` marker means "currently in context": the selected
                    // item in the agent menu, or the running host of the current
                    // chat in the status agents list. Other lists stay flush.
                    let marker = match popup.kind {
                        PopupKind::Agent => Some(app.agent_item_is_effective(category, index)),
                        PopupKind::Status if category == 2 => {
                            Some(app.status_agent_is_current(index))
                        }
                        PopupKind::Status => None,
                    };
                    lines.push(popup_item_line(
                        rows[index].clone(),
                        index == popup.cursor,
                        marker,
                    ));
                }
            }
        }
        PopupLevel::Detail { category, item } => {
            lines.extend(popup_detail_lines(app, popup, category, item));
        }
    }
    lines
}

/// Render the command popup as a panel that fills `area`, covering the lower
/// part of the screen — a modal, not a hint above a still-editable input.
fn render_command_panel(frame: &mut Frame<'_>, area: Rect, lines: Vec<Line<'static>>) {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BRAND))
        .padding(Padding::horizontal(1));
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn popup_breadcrumb(popup: &Popup) -> Line<'static> {
    let mut text = popup.kind.title().to_string();
    if let Some(category) = popup.category() {
        if let Some(label) = popup.kind.categories().get(category) {
            text.push_str(" / ");
            text.push_str(label);
        }
    }
    Line::from(Span::styled(text, muted_style()))
}

fn popup_category_row(label: &str, trailing: Vec<Span<'static>>, selected: bool) -> Line<'static> {
    let label_style = if selected {
        accent_style()
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    };
    let mut spans = vec![
        Span::styled(if selected { "› " } else { "  " }, accent_style()),
        Span::styled(label.to_string(), label_style),
        Span::raw("  "),
    ];
    spans.extend(trailing);
    Line::from(spans)
}

/// Count + a colored health note for a status category, so the categories
/// list surfaces problems at a glance.
fn status_category_trailing(app: &TuiApp, category: usize) -> Vec<Span<'static>> {
    use crate::popup::PopupKind;
    use va_client::runtime::{ChannelStatus, TunnelStatus};
    use va_client::sessions::PtyRunState;

    let count = app.popup_item_count(PopupKind::Status, category);
    let mut spans = vec![Span::styled(count.to_string(), muted_style())];
    let note: Option<(String, Style)> = match category {
        0 => {
            let crashed = app
                .snapshot
                .channels
                .iter()
                .filter(|channel| matches!(channel.status, ChannelStatus::Crashed))
                .count();
            let running = app
                .snapshot
                .channels
                .iter()
                .filter(|channel| matches!(channel.status, ChannelStatus::Running))
                .count();
            health_note(running, crashed, "running", "crashed")
        }
        1 => {
            let failed = app
                .snapshot
                .tunnels
                .iter()
                .filter(|tunnel| matches!(tunnel.status, TunnelStatus::Failed { .. }))
                .count();
            let running = app
                .snapshot
                .tunnels
                .iter()
                .filter(|tunnel| matches!(tunnel.status, TunnelStatus::Running))
                .count();
            health_note(running, failed, "running", "failed")
        }
        2 => {
            let failed = app.snapshot.agents.iter().filter(|a| a.failed.is_some()).count();
            let busy = app.snapshot.agents.iter().filter(|a| a.busy).count();
            if failed > 0 {
                Some((format!("{failed} failed"), Style::default().fg(ERROR)))
            } else if busy > 0 {
                Some((format!("{busy} busy"), Style::default().fg(WARN)))
            } else {
                None
            }
        }
        3 => {
            let running = app
                .snapshot
                .sessions
                .iter()
                .filter(|session| matches!(session.status, PtyRunState::Running { .. }))
                .count();
            (running > 0).then(|| (format!("{running} running"), Style::default().fg(OK)))
        }
        _ => None,
    };
    if let Some((text, style)) = note {
        spans.push(Span::styled(format!("  ·  {text}"), style));
    }
    spans
}

/// Prefer surfacing a problem; otherwise note how many are healthy.
fn health_note(
    healthy: usize,
    problem: usize,
    healthy_label: &str,
    problem_label: &str,
) -> Option<(String, Style)> {
    if problem > 0 {
        Some((
            format!("{problem} {problem_label}"),
            Style::default().fg(ERROR),
        ))
    } else if healthy > 0 {
        Some((
            format!("{healthy} {healthy_label}"),
            Style::default().fg(OK),
        ))
    } else {
        None
    }
}

/// `›` cursor + an optional `●` "currently in context" marker + the item's own
/// spans. `marker` is `None` for read-only lists (status), which keep no marker
/// column at all.
fn popup_item_line(row: Vec<Span<'static>>, selected: bool, marker: Option<bool>) -> Line<'static> {
    let mut spans = vec![Span::styled(if selected { "› " } else { "  " }, accent_style())];
    if let Some(effective) = marker {
        spans.push(Span::styled(if effective { "● " } else { "  " }, accent_style()));
    }
    spans.extend(row);
    let line = Line::from(spans);
    if selected {
        line.style(Style::default().add_modifier(Modifier::BOLD))
    } else {
        line
    }
}

fn agent_session_new_row() -> Vec<Span<'static>> {
    vec![
        Span::styled(format!("{:<10}", "new"), accent_style()),
        Span::styled("start a new session", muted_style()),
    ]
}

fn profile_direct_row() -> Vec<Span<'static>> {
    vec![
        Span::styled(format!("{:<18}", "direct"), accent_style()),
        Span::styled("no managed profile", muted_style()),
    ]
}

fn popup_item_rows(app: &TuiApp, popup: &Popup, category: usize) -> Vec<Vec<Span<'static>>> {
    use crate::popup::PopupKind;
    match popup.kind {
        PopupKind::Status => match category {
            0 => app.snapshot.channels.iter().map(channel_row).collect(),
            1 => app.snapshot.tunnels.iter().map(tunnel_row).collect(),
            2 => app.snapshot.agents.iter().map(agent_row).collect(),
            3 => app.snapshot.sessions.iter().map(session_row).collect(),
            _ => Vec::new(),
        },
        PopupKind::Agent => match category {
            0 => app.agent_picker.agents.iter().map(agent_info_row).collect(),
            // "direct" first, then the managed profiles.
            1 => {
                let mut rows = vec![profile_direct_row()];
                for profile in &app.agent_picker.profiles {
                    rows.push(profile_row(profile));
                }
                rows
            }
            2 => app.agent_picker.workspaces.iter().map(workspace_row).collect(),
            // "new" first, then the sessions for the agent in context.
            3 => {
                let mut rows = vec![agent_session_new_row()];
                for session in app.agent_session_items() {
                    rows.push(launch_session_row(session));
                }
                rows
            }
            _ => Vec::new(),
        },
    }
}

fn popup_detail_lines(
    app: &TuiApp,
    popup: &Popup,
    category: usize,
    item: usize,
) -> Vec<Line<'static>> {
    use crate::popup::PopupKind;
    let detail = match (popup.kind, category) {
        (PopupKind::Status, 0) => app.snapshot.channels.get(item).map(channel_detail),
        (PopupKind::Status, 1) => app.snapshot.tunnels.get(item).map(tunnel_detail),
        (PopupKind::Status, 2) => app.snapshot.agents.get(item).map(agent_detail),
        (PopupKind::Status, 3) => app.snapshot.sessions.get(item).map(session_detail),
        _ => None,
    };
    match detail {
        Some(detail) => {
            let mut lines = vec![Line::from(Span::styled(detail.title, accent_style()))];
            lines.extend(
                detail
                    .lines
                    .into_iter()
                    .map(|text| Line::from(Span::styled(text, muted_style()))),
            );
            lines
        }
        None => vec![Line::from(Span::styled("  no detail", muted_style()))],
    }
}

/// A window of indices that keeps `cursor` visible within `max` rows.
fn popup_window(len: usize, cursor: usize, max: usize) -> std::ops::Range<usize> {
    if len <= max {
        return 0..len;
    }
    let start = cursor.saturating_sub(max / 2).min(len - max);
    start..start + max
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
    // A command popup takes over the lower half of the view: the conversation
    // stays up top, the menu covers the bottom, and the input (and cursor) are
    // hidden so it reads as a modal rather than a still-editable prompt.
    if app.popup.is_some() {
        let lines = command_popup_lines(app);
        let content = u16::try_from(lines.len()).unwrap_or(0).saturating_add(2);
        let panel_height = content
            .max(area.height / 2)
            .min(area.height.saturating_sub(1));
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(panel_height)])
            .split(area);
        render_messages(frame, app, chunks[0]);
        render_command_panel(frame, chunks[1], lines);
        return;
    }

    let input_height =
        input_box_height(&app.chat_input, input_inner_width(area.width), MAX_INPUT_ROWS);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .spacing(1)
        .constraints([Constraint::Min(4), Constraint::Length(input_height)])
        .split(area);
    render_messages(frame, app, chunks[0]);
    render_input_bar(frame, app, chunks[1]);
    render_slash_popup(frame, app, chunks[1]);
}

fn render_messages(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let visible_rows = usize::from(area.height);
    let content_width = usize::from(area.width.saturating_sub(1)).max(1);
    let mut lines = chat_message_lines_for_messages(&app.chat_messages, content_width);
    // A live indicator at the tail so the agent never looks stuck while it
    // thinks or runs tools before any text streams back.
    if let Some(indicator) = working_indicator_line(app) {
        if !lines.is_empty() {
            lines.push(Line::raw(""));
        }
        lines.push(indicator);
    }
    // Conversation flows from the top; once it overflows, the latest lines stay
    // pinned to the bottom via the scroll offset.
    let message_lines = visible_chat_lines(lines, visible_rows, app.chat_scroll);
    frame.render_widget(
        List::new(
            message_lines
                .into_iter()
                .map(ListItem::new)
                .collect::<Vec<_>>(),
        ),
        area,
    );
}

/// Spinner + current activity + elapsed seconds while a turn is in progress.
fn working_indicator_line(app: &TuiApp) -> Option<Line<'static>> {
    let elapsed = app.turn_started_at?.elapsed();
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let frame = (elapsed.as_millis() / 80) as usize % FRAMES.len();
    let activity = app
        .work_status
        .clone()
        .unwrap_or_else(|| "working".to_string());
    Some(Line::from(vec![
        Span::styled(format!("{} ", FRAMES[frame]), accent_style()),
        Span::styled(activity, muted_style()),
        Span::styled(format!("  ·  {}s", elapsed.as_secs()), muted_style()),
    ]))
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
