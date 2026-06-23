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
struct PermissionOption {
    option_id: String,
    name: Option<String>,
}

pub(crate) fn content_text(content: Option<&Value>) -> Option<&str> {
    let content = content?;
    content
        .as_str()
        .or_else(|| content.get("text").and_then(Value::as_str))
}

pub(crate) fn permission_prompt_text(request_id: &str, request: &Value) -> String {
    let options = permission_options(request);
    let option_text = if options.is_empty() {
        "no selectable options".to_string()
    } else {
        options
            .iter()
            .map(|option| match option.name.as_deref() {
                Some(name) if name != option.option_id => format!("{name} ({})", option.option_id),
                _ => option.option_id.clone(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "Permission required: {} [{request_id}]. Options: {option_text}. Use /allow <option-id> or /deny.",
        permission_title(request)
    )
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
    let body_width = marker_body_width(content_width);
    let rows = input
        .split('\n')
        .flat_map(|line| wrap_chat_text_line(line, body_width))
        .count()
        .max(1)
        .min(usize::from(max_body_rows.max(1)));
    u16::try_from(rows)
        .unwrap_or(max_body_rows)
        .saturating_add(2)
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

fn permission_options(request: &Value) -> Vec<PermissionOption> {
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
                    })
                })
                .collect()
        })
        .unwrap_or_default()
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
        assert!(text.contains("Allow (allow-once)"));
        assert!(text.contains("/allow <option-id>"));
    }
}
