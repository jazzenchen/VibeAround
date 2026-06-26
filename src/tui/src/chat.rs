use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::Value;

use crate::theme::{accent_style, muted_style, ACTION, REQUEST_BG};

const CHAT_MARKER_WIDTH: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
struct InputLineSegment {
    text: String,
    start: usize,
    end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChatMessage {
    pub(crate) role: ChatRole,
    pub(crate) text: String,
    pub(crate) work_id: Option<String>,
}

impl ChatMessage {
    pub(crate) fn new(role: ChatRole, text: impl Into<String>) -> Self {
        Self {
            role,
            text: text.into(),
            work_id: None,
        }
    }

    pub(crate) fn work(work_id: Option<String>, text: impl Into<String>) -> Self {
        Self {
            role: ChatRole::Work,
            text: text.into(),
            work_id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChatRole {
    Notice,
    Request,
    Response,
    Work,
}

/// A slash command surfaced in the input autocomplete popup.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SlashCommand {
    pub(crate) name: &'static str,
    pub(crate) summary: &'static str,
}

pub(crate) const SLASH_COMMANDS: &[SlashCommand] = &[
    SlashCommand {
        name: "/new",
        summary: "start the next message in a new session",
    },
    SlashCommand {
        name: "/resume",
        summary: "resume a session by id",
    },
    SlashCommand {
        name: "/status",
        summary: "runtime status",
    },
    SlashCommand {
        name: "/settings",
        summary: "agent context settings",
    },
    SlashCommand {
        name: "/agent",
        summary: "choose agent",
    },
    SlashCommand {
        name: "/profile",
        summary: "choose profile",
    },
    SlashCommand {
        name: "/workspaces",
        summary: "choose workspace",
    },
    SlashCommand {
        name: "/sessions",
        summary: "choose or resume session",
    },
    SlashCommand {
        name: "/mode",
        summary: "list or set the permission mode",
    },
    SlashCommand {
        name: "/clear",
        summary: "clear the conversation",
    },
    SlashCommand {
        name: "/stop",
        summary: "stop the current turn",
    },
    SlashCommand {
        name: "/help",
        summary: "list all commands",
    },
];

/// When the input is a bare command being typed (`/`, `/st`, …) returns the
/// commands whose names share that prefix — the autocomplete popup contents.
/// Returns `None` once the input gains an argument (a space) or isn't a
/// command at all.
pub(crate) fn slash_command_matches(input: &str) -> Option<Vec<&'static SlashCommand>> {
    if !input.starts_with('/') || input.chars().any(char::is_whitespace) {
        return None;
    }
    let matches: Vec<&'static SlashCommand> = SLASH_COMMANDS
        .iter()
        .filter(|command| command.name.starts_with(input))
        .collect();
    if matches.is_empty() {
        None
    } else {
        Some(matches)
    }
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
    let label = tool_activity_label(update).unwrap_or_else(|| "tool".into());
    let kind = tool_activity_kind(update, &label);
    let status = tool_activity_status(update);
    let detail = tool_activity_detail(update, kind);
    let text = match kind {
        ToolActivityKind::Command => match (status, detail) {
            (ToolActivityStatus::Failed, Some(detail)) => format!("command failed: {detail}"),
            (ToolActivityStatus::Completed, Some(detail)) => format!("ran {detail}"),
            (ToolActivityStatus::Active, Some(detail)) => format!("running {detail}"),
            (ToolActivityStatus::Failed, None) => "command failed".to_string(),
            (ToolActivityStatus::Completed, None) => "ran command".to_string(),
            (ToolActivityStatus::Active, None) => "running command".to_string(),
        },
        ToolActivityKind::Search => detail
            .map(|detail| format!("search: {detail}"))
            .unwrap_or_else(|| match status {
                ToolActivityStatus::Failed => "search failed".into(),
                ToolActivityStatus::Completed => "searched".into(),
                ToolActivityStatus::Active => "searching".into(),
            }),
        ToolActivityKind::Edit => detail
            .map(|detail| match status {
                ToolActivityStatus::Active => format!("editing: {detail}"),
                ToolActivityStatus::Failed => format!("edit failed: {detail}"),
                ToolActivityStatus::Completed => format!("edited: {detail}"),
            })
            .unwrap_or_else(|| match status {
                ToolActivityStatus::Failed => "edit failed".into(),
                ToolActivityStatus::Completed => "edited file".into(),
                ToolActivityStatus::Active => "editing file".into(),
            }),
        ToolActivityKind::List => detail
            .map(|detail| format!("list: {detail}"))
            .unwrap_or_else(|| match status {
                ToolActivityStatus::Failed => "list failed".into(),
                ToolActivityStatus::Completed => "listed files".into(),
                ToolActivityStatus::Active => "listing files".into(),
            }),
        ToolActivityKind::File => detail
            .map(|detail| match status {
                ToolActivityStatus::Active => format!("reading: {detail}"),
                ToolActivityStatus::Failed => format!("read failed: {detail}"),
                ToolActivityStatus::Completed => format!("read: {detail}"),
            })
            .unwrap_or_else(|| match status {
                ToolActivityStatus::Failed => "read failed".into(),
                ToolActivityStatus::Completed => "read file".into(),
                ToolActivityStatus::Active => "reading file".into(),
            }),
        ToolActivityKind::Tool => {
            let label = one_line(&label);
            if is_generic_tool_label(&label) {
                match status {
                    ToolActivityStatus::Failed => "tool failed".into(),
                    ToolActivityStatus::Completed | ToolActivityStatus::Active => "working".into(),
                }
            } else {
                match status {
                    ToolActivityStatus::Failed => format!("{label} failed"),
                    ToolActivityStatus::Completed => format!("used {label}"),
                    ToolActivityStatus::Active => format!("using {label}"),
                }
            }
        }
    };
    one_line(&text)
}

pub(crate) fn tool_work_message(update: &Value) -> Option<(Option<String>, String)> {
    let label = tool_activity_label(update).unwrap_or_else(|| "tool".into());
    let kind = tool_activity_kind(update, &label);
    let status = tool_activity_status(update);
    let detail = tool_activity_detail(update, kind);
    let (heading, action) = match kind {
        ToolActivityKind::Command => (
            if status == ToolActivityStatus::Failed {
                "Failed"
            } else {
                "Ran"
            },
            detail.unwrap_or_else(|| "command".into()),
        ),
        ToolActivityKind::Search => (
            if status == ToolActivityStatus::Failed {
                "Failed"
            } else {
                "Explored"
            },
            work_action("Search", detail),
        ),
        ToolActivityKind::List => (
            if status == ToolActivityStatus::Failed {
                "Failed"
            } else {
                "Explored"
            },
            work_action("List", detail),
        ),
        ToolActivityKind::File => (
            if status == ToolActivityStatus::Failed {
                "Failed"
            } else {
                "Explored"
            },
            work_action("Read", detail),
        ),
        ToolActivityKind::Edit => (
            if status == ToolActivityStatus::Failed {
                "Failed"
            } else {
                "Edited"
            },
            work_action("Edit", detail),
        ),
        ToolActivityKind::Tool => {
            let label = one_line(&label);
            if is_generic_tool_label(&label) {
                return None;
            }
            (
                if status == ToolActivityStatus::Failed {
                    "Failed"
                } else {
                    "Used"
                },
                label,
            )
        }
    };
    let mut text = format!("{heading}\n{action}");
    if let Some(output) = tool_activity_output(update) {
        text.push('\n');
        text.push_str("Output ");
        text.push_str(&output);
    }
    Some((tool_activity_id(update), text))
}

fn work_action(verb: &str, detail: Option<String>) -> String {
    match detail {
        Some(detail) => format!("{verb} {detail}"),
        None => verb.to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolActivityKind {
    Command,
    Search,
    Edit,
    List,
    File,
    Tool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolActivityStatus {
    Active,
    Completed,
    Failed,
}

fn tool_activity_label(update: &Value) -> Option<String> {
    let tool_call = update.get("toolCall");
    value_string_field(update, "title")
        .or_else(|| tool_call.and_then(|tool_call| value_string_field(tool_call, "title")))
        .or_else(|| value_string_field(update, "kind"))
        .or_else(|| tool_call.and_then(|tool_call| value_string_field(tool_call, "kind")))
        .or_else(|| value_string_field(update, "name"))
        .or_else(|| value_string_field(update, "toolName"))
        .or_else(|| tool_call.and_then(|tool_call| value_string_field(tool_call, "name")))
}

fn is_generic_tool_label(label: &str) -> bool {
    let normalized = label.trim().to_lowercase();
    normalized.is_empty() || normalized == "tool"
}

fn tool_activity_kind(update: &Value, label: &str) -> ToolActivityKind {
    let tool_call = update.get("toolCall");
    let text = [
        Some(label.to_string()),
        value_string_field(update, "kind"),
        tool_call.and_then(|tool_call| value_string_field(tool_call, "kind")),
        value_string_field(update, "name"),
        value_string_field(update, "toolName"),
        tool_call.and_then(|tool_call| value_string_field(tool_call, "name")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
    .to_lowercase();
    if text.contains("exec")
        || text.contains("command")
        || text.contains("shell")
        || text.contains("stdin")
        || text.contains("terminal")
        || text.contains("bash")
        || tool_activity_field(update, &["command", "cmd", "shell"]).is_some()
    {
        return ToolActivityKind::Command;
    }
    if text.contains("search")
        || text.contains("grep")
        || text.contains("rg")
        || text.contains("find")
        || tool_activity_field(
            update,
            &["query", "q", "pattern", "regex", "search", "keywords"],
        )
        .is_some()
    {
        return ToolActivityKind::Search;
    }
    if text.contains("edit") || text.contains("write") || text.contains("patch") {
        return ToolActivityKind::Edit;
    }
    if text.contains("list")
        || text.contains("ls")
        || text.contains("glob")
        || tool_activity_field(update, &["glob"]).is_some()
    {
        return ToolActivityKind::List;
    }
    if text.contains("read")
        || text.contains("file")
        || text.contains("open")
        || text.contains("view")
        || tool_activity_field(update, &["path", "filePath", "file", "uri"]).is_some()
    {
        return ToolActivityKind::File;
    }
    ToolActivityKind::Tool
}

fn tool_activity_status(update: &Value) -> ToolActivityStatus {
    let tool_call = update.get("toolCall");
    let status = value_string_field(update, "status")
        .or_else(|| tool_call.and_then(|tool_call| value_string_field(tool_call, "status")))
        .or_else(|| value_string_field(update, "state"))
        .or_else(|| value_string_field(update, "outcome"))
        .unwrap_or_default()
        .to_lowercase();
    if status.contains("fail")
        || status.contains("error")
        || status.contains("denied")
        || status.contains("cancel")
    {
        return ToolActivityStatus::Failed;
    }
    if status.contains("complete")
        || status.contains("success")
        || status.contains("succeed")
        || status.contains("done")
    {
        return ToolActivityStatus::Completed;
    }
    ToolActivityStatus::Active
}

fn tool_activity_detail(update: &Value, kind: ToolActivityKind) -> Option<String> {
    match kind {
        ToolActivityKind::Command => tool_activity_command_detail(update),
        ToolActivityKind::Search => tool_activity_search_detail(update),
        ToolActivityKind::Edit => tool_activity_field(
            update,
            &["path", "filePath", "file", "uri", "target", "name"],
        )
        .or_else(|| tool_activity_location(update)),
        ToolActivityKind::List => tool_activity_field(
            update,
            &[
                "path",
                "glob",
                "pattern",
                "directory",
                "dir",
                "folder",
                "target",
            ],
        )
        .or_else(|| tool_activity_location(update)),
        ToolActivityKind::File => tool_activity_field(
            update,
            &["path", "filePath", "file", "uri", "target", "name"],
        )
        .or_else(|| tool_activity_location(update)),
        ToolActivityKind::Tool => tool_activity_field(update, &["query", "path", "file", "name"])
            .or_else(|| tool_activity_location(update)),
    }
}

fn tool_activity_command_detail(update: &Value) -> Option<String> {
    let command = tool_activity_field(update, &["command", "cmd", "shell"])
        .or_else(|| tool_activity_input_text(update))?;
    summarize_command(&command)
}

fn summarize_command(command: &str) -> Option<String> {
    for segment in shell_command_segments(command) {
        let tokens = shell_tokens(&segment);
        if let Some(summary) = summarize_command_tokens(&tokens) {
            return Some(summary);
        }
    }
    None
}

fn summarize_command_tokens(tokens: &[String]) -> Option<String> {
    let mut index = 0;
    while let Some(token) = tokens.get(index).map(String::as_str) {
        if is_command_prefix_token(token) || is_env_assignment(token) {
            index += 1;
            continue;
        }
        if is_shell_program(token) {
            if let Some(script_index) = tokens[index + 1..]
                .iter()
                .position(|arg| matches!(arg.as_str(), "-c" | "-lc" | "-ic" | "-lic"))
                .and_then(|offset| index.checked_add(offset + 2))
            {
                if let Some(script) = tokens.get(script_index) {
                    return summarize_command(script);
                }
            }
        }
        if token == "cd" || token == "export" || token == "source" || token == "." {
            return None;
        }
        let program = command_program_name(token)?;
        let mut parts = vec![program.clone()];
        if let Some(action) = command_action_hint(&program, &tokens[index + 1..]) {
            parts.push(action);
        }
        return Some(parts.join(" "));
    }
    None
}

fn shell_command_segments(command: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    for ch in command.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            current.push(ch);
            continue;
        }
        if let Some(active_quote) = quote {
            current.push(ch);
            if ch == active_quote {
                quote = None;
            }
            continue;
        }
        if matches!(ch, '"' | '\'') {
            quote = Some(ch);
            current.push(ch);
            continue;
        }
        if matches!(ch, ';' | '|' | '&') {
            let segment = current.trim();
            if !segment.is_empty() {
                segments.push(segment.to_string());
            }
            current.clear();
            continue;
        }
        current.push(ch);
    }
    let segment = current.trim();
    if !segment.is_empty() {
        segments.push(segment.to_string());
    }
    segments
}

fn shell_tokens(command: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    for ch in command.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if let Some(active_quote) = quote {
            if ch == active_quote {
                quote = None;
            } else {
                current.push(ch);
            }
            continue;
        }
        if matches!(ch, '"' | '\'') {
            quote = Some(ch);
            continue;
        }
        if ch.is_whitespace() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(ch);
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn is_command_prefix_token(token: &str) -> bool {
    matches!(
        token,
        "sudo" | "command" | "exec" | "noglob" | "time" | "env"
    )
}

fn is_shell_program(token: &str) -> bool {
    matches!(
        command_program_name(token).as_deref(),
        Some("sh" | "bash" | "zsh" | "fish")
    )
}

fn is_env_assignment(token: &str) -> bool {
    let Some((name, _)) = token.split_once('=') else {
        return false;
    };
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn command_program_name(token: &str) -> Option<String> {
    let program = token.rsplit('/').next().unwrap_or(token).trim();
    let program = program.strip_suffix(".exe").unwrap_or(program);
    (!program.is_empty() && !program.starts_with('-')).then(|| one_line(program))
}

fn command_action_hint(program: &str, args: &[String]) -> Option<String> {
    let arg = args
        .iter()
        .find(|arg| !arg.starts_with('-') && !is_env_assignment(arg))?
        .as_str();
    if arg.contains('/') || arg.contains('\\') || arg.contains('.') {
        return None;
    }
    let allow = match program {
        "cargo" => matches!(
            arg,
            "test" | "fmt" | "check" | "clippy" | "build" | "run" | "doc"
        ),
        "git" => matches!(
            arg,
            "status"
                | "diff"
                | "log"
                | "show"
                | "add"
                | "commit"
                | "push"
                | "pull"
                | "fetch"
                | "checkout"
                | "merge"
                | "rebase"
                | "branch"
        ),
        "npm" | "pnpm" | "yarn" | "bun" => matches!(
            arg,
            "run" | "test" | "build" | "dev" | "start" | "install" | "lint"
        ),
        "deno" => matches!(arg, "task" | "test" | "run" | "fmt" | "lint" | "check"),
        _ => false,
    };
    allow.then(|| arg.to_string())
}

fn tool_activity_search_detail(update: &Value) -> Option<String> {
    let query = tool_activity_field(
        update,
        &[
            "query", "q", "pattern", "regex", "search", "keywords", "text",
        ],
    );
    let site = tool_activity_field(update, &["site", "domain", "domains", "source", "url"]);
    match (query, site) {
        (Some(query), Some(site)) if !query.contains(&site) => Some(format!("{query} ({site})")),
        (Some(query), _) => Some(query),
        (_, Some(site)) => Some(site),
        _ => tool_activity_input_text(update),
    }
}

fn tool_activity_field(update: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(text) = detail_field(update, key) {
            return Some(text);
        }
        if let Some(text) = update
            .get("toolCall")
            .and_then(|tool_call| detail_field(tool_call, key))
        {
            return Some(text);
        }
    }
    for container_key in ["rawInput", "raw_input", "input", "arguments", "params"] {
        if let Some(text) = update
            .get(container_key)
            .and_then(|container| detail_field_any(container, keys))
        {
            return Some(text);
        }
        if let Some(text) = update
            .get("toolCall")
            .and_then(|tool_call| tool_call.get(container_key))
            .and_then(|container| detail_field_any(container, keys))
        {
            return Some(text);
        }
    }
    None
}

fn detail_field_any(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(text) = detail_field(value, key) {
            return Some(text);
        }
    }
    None
}

fn detail_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(detail_value_text)
}

fn detail_value_text(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(one_line(text)).filter(|text| !text.is_empty());
    }
    if value.is_number() || value.is_boolean() {
        return Some(value.to_string());
    }
    if let Some(items) = value.as_array() {
        let text = items
            .iter()
            .filter_map(detail_value_text)
            .take(3)
            .collect::<Vec<_>>()
            .join(", ");
        return (!text.is_empty()).then_some(text);
    }
    if value.is_object() {
        return detail_field_any(
            value,
            &[
                "query", "q", "pattern", "regex", "path", "filePath", "file", "uri", "url", "site",
                "domain", "name", "text",
            ],
        );
    }
    None
}

fn tool_activity_input_text(update: &Value) -> Option<String> {
    for container_key in ["rawInput", "raw_input", "input", "arguments", "params"] {
        if let Some(text) = update.get(container_key).and_then(detail_value_text) {
            return Some(text);
        }
        if let Some(text) = update
            .get("toolCall")
            .and_then(|tool_call| tool_call.get(container_key))
            .and_then(detail_value_text)
        {
            return Some(text);
        }
    }
    None
}

fn tool_activity_output(update: &Value) -> Option<String> {
    for key in ["output", "rawOutput", "raw_output", "result"] {
        if let Some(text) = update.get(key).and_then(short_output_text) {
            return Some(text);
        }
        if let Some(text) = update
            .get("toolCall")
            .and_then(|tool_call| tool_call.get(key))
            .and_then(short_output_text)
        {
            return Some(text);
        }
    }
    None
}

fn short_output_text(value: &Value) -> Option<String> {
    let text = value.as_str()?;
    let text = one_line(text);
    if text.is_empty() || text.chars().count() > 100 {
        return None;
    }
    Some(text)
}

fn tool_activity_location(update: &Value) -> Option<String> {
    location_from_value(update).or_else(|| update.get("toolCall").and_then(location_from_value))
}

fn location_from_value(value: &Value) -> Option<String> {
    value
        .get("locations")
        .and_then(Value::as_array)
        .and_then(|locations| locations.first())
        .and_then(|location| {
            let path = value_string_field(location, "path")
                .or_else(|| value_string_field(location, "uri"))?;
            let line = location
                .get("line")
                .and_then(Value::as_u64)
                .map(|line| format!(":{line}"))
                .unwrap_or_default();
            Some(format!("{path}{line}"))
        })
        .or_else(|| {
            value.get("location").and_then(|location| {
                let path = value_string_field(location, "path")
                    .or_else(|| value_string_field(location, "uri"))?;
                let line = location
                    .get("line")
                    .and_then(Value::as_u64)
                    .map(|line| format!(":{line}"))
                    .unwrap_or_default();
                Some(format!("{path}{line}"))
            })
        })
}

fn tool_activity_id(update: &Value) -> Option<String> {
    value_string_field(update, "toolCallId")
        .or_else(|| value_string_field(update, "tool_call_id"))
        .or_else(|| value_string_field(update, "id"))
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
    let rows = input_visible_lines(input, input.len(), content_width, max_body_rows).len();
    u16::try_from(rows)
        .unwrap_or(max_body_rows)
        .max(1)
        .saturating_add(2)
}

pub(crate) fn input_visible_lines(
    input: &str,
    cursor: usize,
    content_width: usize,
    max_body_rows: u16,
) -> Vec<String> {
    let (segments, start, _) = input_visible_segments(input, cursor, content_width, max_body_rows);
    let max_body_rows = usize::from(max_body_rows.max(1));
    segments
        .into_iter()
        .skip(start)
        .take(max_body_rows)
        .map(|segment| segment.text)
        .collect()
}

pub(crate) fn input_cursor_offset(
    input: &str,
    cursor: usize,
    content_width: usize,
    max_body_rows: u16,
) -> (u16, u16) {
    let cursor = clamp_input_cursor(input, cursor);
    let (segments, start, cursor_line) =
        input_visible_segments(input, cursor, content_width, max_body_rows);
    let segment = segments.get(cursor_line);
    let max_x = content_width.saturating_sub(1);
    let cursor_text = segment
        .and_then(|segment| input.get(segment.start..cursor))
        .unwrap_or("");
    let cursor_x = CHAT_MARKER_WIDTH
        .saturating_add(display_width(cursor_text))
        .min(max_x);
    let cursor_y = cursor_line.saturating_sub(start);
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
    let command_action_line = (message.role == ChatRole::Work)
        .then(|| work_command_action_line(&message.text))
        .flatten();
    let mut lines = Vec::new();
    for (raw_index, raw_text) in message.text.split('\n').enumerate() {
        let command_action = command_action_line == Some(raw_index);
        for text in wrap_chat_text_line(raw_text, body_width) {
            let first_line = lines.is_empty();
            lines.push(chat_message_line_at(
                message.role,
                &text,
                first_line,
                content_width,
                command_action,
            ));
        }
    }
    lines
}

#[cfg(test)]
fn chat_message_line(message: &ChatMessage) -> Line<'static> {
    chat_message_line_at(message.role, &message.text, true, usize::MAX, false)
}

fn chat_message_line_at(
    role: ChatRole,
    text: &str,
    first_line: bool,
    content_width: usize,
    command_action: bool,
) -> Line<'static> {
    let marker = match role {
        ChatRole::Notice => "· ",
        ChatRole::Request => "› ",
        ChatRole::Response => "• ",
        ChatRole::Work => "• ",
    };
    let marker = if first_line { marker } else { "  " };
    match role {
        ChatRole::Notice => Line::from(vec![
            Span::styled(marker, muted_style()),
            Span::raw(text.to_string()),
        ]),
        ChatRole::Request => request_line(marker, text, content_width),
        ChatRole::Response => Line::from(vec![
            Span::styled(marker, Style::default()),
            Span::raw(text.to_string()),
        ]),
        ChatRole::Work => work_line(marker, text, first_line, command_action),
    }
}

fn work_line(marker: &str, text: &str, first_line: bool, command_action: bool) -> Line<'static> {
    if first_line {
        return Line::from(vec![
            Span::styled(marker.to_string(), Style::default().fg(ACTION)),
            Span::styled(
                text.to_string(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]);
    }
    if command_action {
        return Line::from(vec![
            Span::styled("  └ ", muted_style()),
            Span::styled(text.to_string(), muted_style()),
        ]);
    }
    let (verb, rest) = text.split_once(' ').unwrap_or((text, ""));
    let mut spans = vec![
        Span::styled("  └ ", muted_style()),
        Span::styled(verb.to_string(), accent_style()),
    ];
    if !rest.is_empty() {
        spans.push(Span::raw(format!(" {rest}")));
    }
    Line::from(spans)
}

fn work_command_action_line(text: &str) -> Option<usize> {
    let mut lines = text.split('\n');
    let heading = lines.next()?;
    let action = lines.next()?;
    if matches!(heading, "Ran" | "Failed") && is_command_work_action(action) {
        Some(1)
    } else {
        None
    }
}

fn is_command_work_action(action: &str) -> bool {
    let first = action.split_whitespace().next().unwrap_or_default();
    !matches!(first, "Search" | "List" | "Read" | "Edit" | "Output")
}

fn request_line(marker: &str, text: &str, content_width: usize) -> Line<'static> {
    let body_style = Style::default().bg(REQUEST_BG);
    let marker_style = body_style.fg(ACTION).add_modifier(Modifier::BOLD);
    let mut spans = vec![
        Span::styled(marker.to_string(), marker_style),
        Span::styled(text.to_string(), body_style),
    ];
    if content_width != usize::MAX {
        let line_width = CHAT_MARKER_WIDTH.saturating_add(display_width(text));
        if content_width > line_width {
            spans.push(Span::styled(
                " ".repeat(content_width - line_width),
                body_style,
            ));
        }
    }
    Line::from(spans).style(body_style)
}

fn marker_body_width(content_width: usize) -> usize {
    if content_width == usize::MAX {
        return content_width;
    }
    content_width.saturating_sub(CHAT_MARKER_WIDTH).max(1)
}

fn input_visible_segments(
    input: &str,
    cursor: usize,
    content_width: usize,
    max_body_rows: u16,
) -> (Vec<InputLineSegment>, usize, usize) {
    let segments = input_wrapped_segments(input, content_width);
    let cursor = clamp_input_cursor(input, cursor);
    let cursor_line = cursor_segment_index(&segments, cursor);
    let max_body_rows = usize::from(max_body_rows.max(1));
    let start = cursor_line.saturating_sub(max_body_rows.saturating_sub(1));
    (segments, start, cursor_line)
}

fn input_wrapped_segments(input: &str, content_width: usize) -> Vec<InputLineSegment> {
    let body_width = marker_body_width(content_width);
    let mut segments = Vec::new();
    let mut line_start = 0;
    for line in input.split('\n') {
        segments.extend(wrap_input_text_line(line, line_start, body_width));
        line_start += line.len() + 1;
    }
    if segments.is_empty() {
        segments.push(InputLineSegment {
            text: String::new(),
            start: 0,
            end: 0,
        });
    }
    segments
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

fn wrap_input_text_line(
    text: &str,
    line_start: usize,
    content_width: usize,
) -> Vec<InputLineSegment> {
    if text.is_empty() || content_width == 0 || content_width == usize::MAX {
        return vec![InputLineSegment {
            text: text.to_string(),
            start: line_start,
            end: line_start + text.len(),
        }];
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_start = line_start;
    let mut current_end = line_start;
    for (relative_index, ch) in text.char_indices() {
        let absolute_index = line_start + relative_index;
        let mut candidate = current.clone();
        candidate.push(ch);
        if !current.is_empty() && display_width(&candidate) > content_width {
            lines.push(InputLineSegment {
                text: current,
                start: current_start,
                end: current_end,
            });
            current = ch.to_string();
            current_start = absolute_index;
        } else {
            current = candidate;
        }
        current_end = absolute_index + ch.len_utf8();
    }
    lines.push(InputLineSegment {
        text: current,
        start: current_start,
        end: current_end,
    });
    lines
}

fn cursor_segment_index(segments: &[InputLineSegment], cursor: usize) -> usize {
    segments
        .iter()
        .position(|segment| cursor >= segment.start && cursor < segment.end)
        .or_else(|| {
            segments
                .iter()
                .position(|segment| cursor == segment.start && segment.start == segment.end)
        })
        .or_else(|| segments.iter().rposition(|segment| cursor == segment.end))
        .unwrap_or_else(|| segments.len().saturating_sub(1))
}

fn clamp_input_cursor(input: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(input.len());
    while cursor > 0 && !input.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
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
        let request =
            row_text(chat_message_line(&ChatMessage::new(ChatRole::Request, "hello")).spans);
        let response =
            row_text(chat_message_line(&ChatMessage::new(ChatRole::Response, "hi")).spans);
        let notice =
            row_text(chat_message_line(&ChatMessage::new(ChatRole::Notice, "ready")).spans);

        assert_eq!(request, "› hello");
        assert_eq!(response, "• hi");
        assert_eq!(notice, "· ready");
        assert!(!request.contains("you"));
        assert!(!notice.contains("system"));
    }

    #[test]
    fn work_items_render_codex_style_rows() {
        let lines = chat_message_lines(&ChatMessage::work(
            Some("call-read".into()),
            "Explored\nRead session_index.jsonl",
        ))
        .into_iter()
        .map(|line| row_text(line.spans))
        .collect::<Vec<_>>();

        assert_eq!(lines, vec!["• Explored", "  └ Read session_index.jsonl"]);
    }

    #[test]
    fn command_work_detail_uses_secondary_style() {
        let lines = chat_message_lines(&ChatMessage::work(
            Some("call-command".into()),
            "Ran\ncargo test",
        ));

        assert_eq!(row_text(lines[1].spans.clone()), "  └ cargo test");
        assert_eq!(lines[1].spans[1].style, muted_style());
    }

    #[test]
    fn chat_multiline_messages_keep_raw_lines_with_continuation_indent() {
        let lines = chat_message_lines(&ChatMessage::new(
            ChatRole::Response,
            "hello **raw**\n\nworld",
        ))
        .into_iter()
        .map(|line| row_text(line.spans))
        .collect::<Vec<_>>();

        assert_eq!(lines, vec!["• hello **raw**", "", "  world"]);
    }

    #[test]
    fn chat_lines_soft_wrap_to_available_width() {
        let lines = chat_message_wrapped_lines(&ChatMessage::new(ChatRole::Request, "abcdef"), 5)
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
            &ChatMessage::new(ChatRole::Response, "abcdefghijkl"),
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
            input_visible_lines("abcdef", "abcdef".len(), 5, 4),
            vec!["abc".to_string(), "def".to_string()]
        );
        assert_eq!(
            input_visible_lines("a\nb\nc\nd\ne", "a\nb\nc\nd\ne".len(), 10, 4),
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
        assert_eq!(input_cursor_offset("", 0, 10, 4), (2, 0));
        assert_eq!(input_cursor_offset("abc", "abc".len(), 10, 4), (5, 0));
        assert_eq!(
            input_cursor_offset("abc\ndef", "abc\ndef".len(), 10, 4),
            (5, 1)
        );
        assert_eq!(input_cursor_offset("abcdef", "abcdef".len(), 5, 4), (4, 1));
    }

    #[test]
    fn input_visible_lines_follow_cursor_window() {
        assert_eq!(
            input_visible_lines("a\nb\nc\nd\ne", 0, 10, 4),
            vec![
                "a".to_string(),
                "b".to_string(),
                "c".to_string(),
                "d".to_string()
            ]
        );
        assert_eq!(input_cursor_offset("abcdef", 3, 5, 4), (2, 1));
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
    fn tool_activity_summarizes_search_query() {
        let update = serde_json::json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call-123",
            "kind": "search",
            "status": "completed",
            "rawInput": {
                "query": "rust ratatui render line"
            }
        });

        assert_eq!(
            tool_activity_text(&update),
            "search: rust ratatui render line"
        );
    }

    #[test]
    fn tool_activity_summarizes_command_without_private_args() {
        let update = serde_json::json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call-cmd",
            "kind": "exec_command",
            "status": "completed",
            "rawInput": {
                "command": "cat /very/private/file"
            }
        });

        let text = tool_activity_text(&update);
        assert_eq!(text, "ran cat");
        assert!(!text.contains("private"));
    }

    #[test]
    fn tool_activity_summarizes_common_command_subcommand() {
        let update = serde_json::json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call-cmd",
            "kind": "exec_command",
            "status": "completed",
            "rawInput": {
                "command": "cd src && cargo test -p va-tui"
            }
        });

        assert_eq!(tool_activity_text(&update), "ran cargo test");
    }

    #[test]
    fn tool_work_message_summarizes_without_raw_payloads() {
        let update = serde_json::json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call-search",
            "kind": "search",
            "status": "completed",
            "rawInput": {
                "query": "rust ratatui render line"
            },
            "rawOutput": "3 matches"
        });

        assert_eq!(
            tool_work_message(&update),
            Some((
                Some("call-search".into()),
                "Explored\nSearch rust ratatui render line\nOutput 3 matches".into()
            ))
        );
    }

    #[test]
    fn tool_work_message_hides_command_body() {
        let update = serde_json::json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call-cmd",
            "kind": "exec_command",
            "status": "completed",
            "rawInput": {
                "command": "cat /very/private/file"
            }
        });

        let (_, text) = tool_work_message(&update).expect("work message");
        assert_eq!(text, "Ran\ncat");
        assert!(!text.contains("private"));
    }

    #[test]
    fn tool_activity_detects_generic_command_input() {
        let update = serde_json::json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call-cmd",
            "title": "tool",
            "status": "running",
            "rawInput": {
                "command": "cat /very/private/file"
            }
        });

        let text = tool_activity_text(&update);
        assert_eq!(text, "running cat");
        assert!(!text.contains("private"));
    }

    #[test]
    fn tool_activity_summarizes_read_path() {
        let update = serde_json::json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call-read",
            "kind": "read",
            "status": "completed",
            "rawInput": {
                "path": "src/tui/src/chat.rs"
            }
        });

        assert_eq!(tool_activity_text(&update), "read: src/tui/src/chat.rs");
    }

    #[test]
    fn tool_activity_hides_bare_tool_call_id() {
        let update = serde_json::json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call-abcdef123456",
            "status": "running"
        });

        assert_eq!(tool_activity_text(&update), "working");
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
