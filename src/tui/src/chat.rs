use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::Value;

use crate::theme::{muted_style, BRAND};

const CHAT_MARKER_WIDTH: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChatMessage {
    pub(crate) role: ChatRole,
    pub(crate) text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChatRole {
    Notice,
    Request,
    Response,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PermissionOption {
    pub(crate) option_id: String,
    pub(crate) name: Option<String>,
    pub(crate) kind: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionModeSource {
    ConfigOption,
    SessionMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionModeState {
    pub(crate) source: SessionModeSource,
    pub(crate) config_id: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) current_value: String,
    pub(crate) options: Vec<SessionModeOption>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionModeOption {
    pub(crate) value: String,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) group: Option<String>,
}

pub(crate) fn content_text(content: Option<&Value>) -> Option<&str> {
    let content = content?;
    content
        .as_str()
        .or_else(|| content.get("text").and_then(Value::as_str))
}

pub(crate) fn parse_session_mode_state(value: Option<&Value>) -> Option<SessionModeState> {
    let value = value?;
    let source = match value_string_field(value, "source")?.as_str() {
        "config_option" => SessionModeSource::ConfigOption,
        "session_mode" => SessionModeSource::SessionMode,
        _ => return None,
    };
    let current_value = value_string_field(value, "currentValue")
        .or_else(|| value_string_field(value, "currentModeId"))
        .or_else(|| value_string_field(value, "modeId"))
        .or_else(|| value_string_field(value, "mode_id"))?;
    let options = session_mode_options(value.get("options"));
    if options.is_empty() {
        return None;
    }
    Some(SessionModeState {
        source,
        config_id: value_string_field(value, "configId"),
        name: value_string_field(value, "name"),
        current_value,
        options,
    })
}

pub(crate) fn session_mode_display_label(value: Option<&Value>) -> Option<String> {
    let state = parse_session_mode_state(value)?;
    state
        .options
        .iter()
        .find(|option| option.value == state.current_value)
        .map(|option| option.name.clone())
        .or(Some(state.current_value))
}

pub(crate) fn session_mode_options_text(value: Option<&Value>) -> Option<String> {
    let state = parse_session_mode_state(value)?;
    let label = state.name.as_deref().unwrap_or("Session mode");
    let options = state
        .options
        .iter()
        .enumerate()
        .map(|(index, option)| {
            let current = if option.value == state.current_value {
                " *"
            } else {
                ""
            };
            format!("{} {} ({}){current}", index + 1, option.name, option.value)
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some(format!("{label}\n{options}\nUse /mode <number|value>."))
}

pub(crate) fn resolve_session_mode_value(value: Option<&Value>, selector: &str) -> Option<String> {
    let state = parse_session_mode_state(value)?;
    let selector = selector.trim();
    if selector.is_empty() {
        return None;
    }
    if let Some(index) = selector
        .parse::<usize>()
        .ok()
        .and_then(|index| index.checked_sub(1))
    {
        return state.options.get(index).map(|option| option.value.clone());
    }
    state
        .options
        .iter()
        .find(|option| option.value == selector || option.name.eq_ignore_ascii_case(selector))
        .map(|option| option.value.clone())
}

pub(crate) fn permission_prompt_text(request_id: &str, request: &Value) -> String {
    let options = permission_options(request);
    let option_text = if options.is_empty() {
        "no selectable options".to_string()
    } else {
        options
            .iter()
            .enumerate()
            .map(|(index, option)| match option.name.as_deref() {
                Some(name) if name != option.option_id => {
                    format!("{} {name} ({})", index + 1, option.option_id)
                }
                _ => format!("{} {}", index + 1, option.option_id),
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "Permission required: {} [{request_id}]\n{option_text}\nUse /allow [number|option-id] or /deny.",
        permission_title(request)
    )
}

pub(crate) fn resolve_permission_option(request: &Value, selector: Option<&str>) -> Option<String> {
    let options = permission_options(request);
    if options.is_empty() {
        return None;
    }
    let selector = selector.map(str::trim).filter(|value| !value.is_empty());
    let Some(selector) = selector else {
        return default_permission_option(&options).map(|option| option.option_id.clone());
    };
    if let Some(option) = options.iter().find(|option| option.option_id == selector) {
        return Some(option.option_id.clone());
    }
    let index = selector.parse::<usize>().ok()?.checked_sub(1)?;
    options.get(index).map(|option| option.option_id.clone())
}

pub(crate) fn tool_activity_text(update: &Value) -> String {
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

pub(crate) fn one_line(value: &str) -> String {
    let text = value.split_whitespace().collect::<Vec<_>>().join(" ");
    const LIMIT: usize = 120;
    if text.chars().count() <= LIMIT {
        return text;
    }
    let mut truncated = text.chars().take(LIMIT).collect::<String>();
    truncated.push('…');
    truncated
}

pub(crate) fn chat_message_lines_for_messages(
    messages: &[ChatMessage],
    content_width: usize,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        if index > 0 {
            lines.push(Line::raw(""));
        }
        lines.extend(chat_message_wrapped_lines(message, content_width));
    }
    lines
}

pub(crate) fn visible_chat_lines(
    lines: Vec<Line<'static>>,
    capacity: usize,
    scroll_from_bottom: usize,
) -> Vec<Line<'static>> {
    if capacity == 0 || lines.len() <= capacity {
        return lines;
    }
    let max_start = lines.len().saturating_sub(capacity);
    let start = max_start.saturating_sub(scroll_from_bottom.min(max_start));
    lines.into_iter().skip(start).take(capacity).collect()
}

pub(crate) fn input_box_height(input: &str, content_width: usize, max_body_rows: u16) -> u16 {
    let rows = input_visible_lines(input, content_width, max_body_rows).len();
    u16::try_from(rows)
        .unwrap_or(max_body_rows)
        .saturating_add(2)
}

pub(crate) fn input_visible_lines(
    input: &str,
    content_width: usize,
    max_body_rows: u16,
) -> Vec<String> {
    let max_body_rows = usize::from(max_body_rows.max(1));
    let lines = input_wrapped_lines(input, content_width);
    let start = lines.len().saturating_sub(max_body_rows);
    lines.into_iter().skip(start).collect()
}

pub(crate) fn input_cursor_offset(
    input: &str,
    content_width: usize,
    max_body_rows: u16,
) -> (u16, u16) {
    let lines = input_visible_lines(input, content_width, max_body_rows);
    let last_line = lines.last().map(String::as_str).unwrap_or("");
    let max_x = content_width.saturating_sub(1);
    let cursor_x = CHAT_MARKER_WIDTH
        .saturating_add(display_width(last_line))
        .min(max_x);
    let cursor_y = lines.len().saturating_sub(1);
    (
        u16::try_from(cursor_x).unwrap_or(u16::MAX),
        u16::try_from(cursor_y).unwrap_or(u16::MAX),
    )
}

#[cfg(test)]
fn chat_message_lines(message: &ChatMessage) -> Vec<Line<'static>> {
    chat_message_wrapped_lines(message, usize::MAX)
}

fn chat_message_wrapped_lines(message: &ChatMessage, content_width: usize) -> Vec<Line<'static>> {
    let body_width = marker_body_width(content_width);
    message
        .text
        .split('\n')
        .flat_map(|text| wrap_chat_text_line(text, body_width))
        .enumerate()
        .map(|(index, text)| chat_message_line_at(message.role, &text, index == 0))
        .collect()
}

#[cfg(test)]
fn chat_message_line(message: &ChatMessage) -> Line<'static> {
    chat_message_line_at(message.role, &message.text, true)
}

fn chat_message_line_at(role: ChatRole, text: &str, first_line: bool) -> Line<'static> {
    let (marker, style) = match role {
        ChatRole::Notice => ("* ", muted_style()),
        ChatRole::Request => (
            "› ",
            Style::default().fg(BRAND).add_modifier(Modifier::BOLD),
        ),
        ChatRole::Response => ("• ", Style::default()),
    };
    let marker = if first_line { marker } else { "  " };
    Line::from(vec![
        Span::styled(marker, style),
        Span::raw(text.to_string()),
    ])
}

fn marker_body_width(content_width: usize) -> usize {
    if content_width == usize::MAX {
        return content_width;
    }
    content_width.saturating_sub(CHAT_MARKER_WIDTH).max(1)
}

fn input_wrapped_lines(input: &str, content_width: usize) -> Vec<String> {
    let body_width = marker_body_width(content_width);
    input
        .split('\n')
        .flat_map(|line| wrap_chat_text_line(line, body_width))
        .collect::<Vec<_>>()
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

pub(crate) fn permission_options(request: &Value) -> Vec<PermissionOption> {
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
                        kind: value_string_field(item, "kind"),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn default_permission_option(options: &[PermissionOption]) -> Option<&PermissionOption> {
    options
        .iter()
        .find(|option| {
            !option
                .kind
                .as_deref()
                .unwrap_or_default()
                .starts_with("reject")
        })
        .or_else(|| options.first())
}

fn session_mode_options(value: Option<&Value>) -> Vec<SessionModeOption> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };
    collect_session_mode_options(items, None)
}

fn collect_session_mode_options(items: &[Value], group: Option<&str>) -> Vec<SessionModeOption> {
    items
        .iter()
        .flat_map(|item| {
            if let Some(Value::Array(children)) = item.get("options") {
                let child_group = value_string_field(item, "name");
                collect_session_mode_options(children, child_group.as_deref().or(group))
            } else {
                let Some(value) = value_string_field(item, "value") else {
                    return Vec::new();
                };
                let Some(name) = value_string_field(item, "name") else {
                    return Vec::new();
                };
                vec![SessionModeOption {
                    value,
                    name,
                    description: value_string_field(item, "description"),
                    group: value_string_field(item, "group")
                        .or_else(|| group.map(ToOwned::to_owned)),
                }]
            }
        })
        .collect()
}

fn value_string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn wrap_chat_text_line(text: &str, content_width: usize) -> Vec<String> {
    if text.is_empty() || content_width == 0 || content_width == usize::MAX {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        let mut candidate = current.clone();
        candidate.push(ch);
        if !current.is_empty() && display_width(&candidate) > content_width {
            lines.push(current);
            current = ch.to_string();
        } else {
            current = candidate;
        }
    }
    lines.push(current);
    lines
}

fn display_width(text: &str) -> usize {
    Line::from(text.to_string()).width()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row_text(row: Vec<Span<'static>>) -> String {
        row.into_iter()
            .map(|span| span.content.into_owned())
            .collect::<String>()
            .trim_end()
            .to_string()
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
    fn chat_multiline_messages_keep_raw_lines_with_continuation_indent() {
        let lines = chat_message_lines(&ChatMessage {
            role: ChatRole::Response,
            text: "hello **raw**\n\nworld".into(),
        })
        .into_iter()
        .map(|line| row_text(line.spans))
        .collect::<Vec<_>>();

        assert_eq!(lines, vec!["• hello **raw**", "", "  world"]);
    }

    #[test]
    fn chat_lines_soft_wrap_to_available_width() {
        let lines = chat_message_wrapped_lines(
            &ChatMessage {
                role: ChatRole::Request,
                text: "abcdef".into(),
            },
            5,
        )
        .into_iter()
        .map(|line| {
            let width = line.width();
            let text = row_text(line.spans);
            (text, width)
        })
        .collect::<Vec<_>>();

        assert_eq!(
            lines,
            vec![("› abc".to_string(), 5), ("  def".to_string(), 5)]
        );
    }

    #[test]
    fn chat_lines_never_exceed_available_width() {
        let max_width = 8;
        let lines = chat_message_wrapped_lines(
            &ChatMessage {
                role: ChatRole::Response,
                text: "abcdefghijkl".into(),
            },
            max_width,
        );

        assert!(lines.iter().all(|line| line.width() <= max_width));
    }

    #[test]
    fn input_box_height_grows_with_wrapped_input() {
        assert_eq!(input_box_height("", 10, 4), 3);
        assert_eq!(input_box_height("abcdefghijkl", 8, 4), 4);
        assert_eq!(input_box_height("abcdefghijklmnop", 4, 4), 6);
    }

    #[test]
    fn input_visible_lines_wrap_and_follow_tail() {
        assert_eq!(
            input_visible_lines("abcdef", 5, 4),
            vec!["abc".to_string(), "def".to_string()]
        );
        assert_eq!(
            input_visible_lines("a\nb\nc\nd\ne", 10, 4),
            vec![
                "b".to_string(),
                "c".to_string(),
                "d".to_string(),
                "e".to_string()
            ]
        );
    }

    #[test]
    fn input_cursor_offset_tracks_visible_input_end() {
        assert_eq!(input_cursor_offset("", 10, 4), (2, 0));
        assert_eq!(input_cursor_offset("abc", 10, 4), (5, 0));
        assert_eq!(input_cursor_offset("abc\ndef", 10, 4), (5, 1));
        assert_eq!(input_cursor_offset("abcdef", 5, 4), (4, 1));
    }

    #[test]
    fn visible_chat_lines_follow_tail_and_support_scrollback() {
        let lines = (0..6)
            .map(|index| Line::from(index.to_string()))
            .collect::<Vec<_>>();

        let bottom = visible_chat_lines(lines.clone(), 3, 0)
            .into_iter()
            .map(|line| row_text(line.spans))
            .collect::<Vec<_>>();
        let scrolled = visible_chat_lines(lines, 3, 2)
            .into_iter()
            .map(|line| row_text(line.spans))
            .collect::<Vec<_>>();

        assert_eq!(bottom, vec!["3", "4", "5"]);
        assert_eq!(scrolled, vec!["1", "2", "3"]);
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
        assert!(text.contains("Permission required: Read [req-1]\n1 Allow (allow-once)"));
        assert!(text.contains("Allow (allow-once)"));
        assert!(text.contains("2 Reject (reject)\nUse /allow [number|option-id]"));
    }

    #[test]
    fn permission_options_can_be_selected_by_default_number_or_id() {
        let request = serde_json::json!({
            "toolCall": { "title": "Read" },
            "options": [
                { "optionId": "reject", "name": "Reject", "kind": "reject" },
                { "optionId": "allow-once", "name": "Allow" },
                { "optionId": "allow-always", "name": "Always allow" }
            ]
        });

        assert_eq!(
            resolve_permission_option(&request, None).as_deref(),
            Some("allow-once")
        );
        assert_eq!(
            resolve_permission_option(&request, Some("3")).as_deref(),
            Some("allow-always")
        );
        assert_eq!(
            resolve_permission_option(&request, Some("reject")).as_deref(),
            Some("reject")
        );
        assert_eq!(resolve_permission_option(&request, Some("9")), None);
    }

    #[test]
    fn session_mode_state_parses_nested_options() {
        let value = serde_json::json!({
            "source": "config_option",
            "configId": "permissions",
            "name": "Permission mode",
            "currentValue": "acceptEdits",
            "options": [
                { "name": "Safe", "options": [
                    { "value": "default", "name": "Default" },
                    { "value": "acceptEdits", "name": "Accept edits" }
                ]},
                { "value": "bypassPermissions", "name": "Bypass permissions" }
            ]
        });

        let state = parse_session_mode_state(Some(&value)).expect("mode state");

        assert_eq!(state.source, SessionModeSource::ConfigOption);
        assert_eq!(state.config_id.as_deref(), Some("permissions"));
        assert_eq!(state.current_value, "acceptEdits");
        assert_eq!(state.options.len(), 3);
        assert_eq!(state.options[1].name, "Accept edits");
        assert_eq!(state.options[1].group.as_deref(), Some("Safe"));
        assert_eq!(
            session_mode_display_label(Some(&value)).as_deref(),
            Some("Accept edits")
        );
        assert_eq!(
            resolve_session_mode_value(Some(&value), "2").as_deref(),
            Some("acceptEdits")
        );
        assert_eq!(
            resolve_session_mode_value(Some(&value), "Bypass permissions").as_deref(),
            Some("bypassPermissions")
        );
        let options_text = session_mode_options_text(Some(&value)).unwrap();
        assert!(options_text.contains("Permission mode\n1 Default (default)"));
        assert!(options_text.contains("2 Accept edits (acceptEdits) *"));
        assert!(options_text.contains("3 Bypass permissions (bypassPermissions)"));
        assert!(options_text.ends_with("Use /mode <number|value>."));
    }
}
