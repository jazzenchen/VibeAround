//! Claude CLI headless runner: run a prompt via `claude -p "..."` and return full text or stream segments.
//! Parses stream-json: content_block_start, content_block_delta, content_block_stop, message_stop.
//! Segments are organized by Claude type and stop markers (one TextPart per text block).
//! Implements HeadlessRunner for unified dispatch.

use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;

use crate::headless::{self, chat_working_dir};

/// Progress event from Claude stream-json. Use to show "Thinking...", "Using tool: X" on Telegram.
#[derive(Debug, Clone, PartialEq)]
pub enum ClaudeProgress {
    /// Extended thinking block started (model is reasoning).
    Thinking,
    /// Tool use block started (model is calling a tool).
    ToolUse { name: String },
}

/// One segment from the Claude stream: either a progress label or one text block (between content_block_stop boundaries).
#[derive(Debug, Clone)]
pub enum ClaudeSegment {
    Progress(ClaudeProgress),
    TextPart(String),
}

/// Parsed stream line: progress, text_delta, stop mark, and optional debug info (block type / delta content for logging).
pub struct StreamParseResult {
    pub progress: Option<ClaudeProgress>,
    pub text_delta: Option<String>,
    pub mark: Option<StreamEventMark>,
    /// Block type from content_block_start (e.g. "text", "tool_use", "thinking") for debug dump on block_stop.
    pub block_type: Option<String>,
    /// Delta content to accumulate for current block (text_delta, input_json_delta, thinking_delta) for debug dump on block_stop.
    pub delta_for_debug: Option<String>,
}

/// Result of parsing one stream-json line: progress, text_delta, stop mark, and optional block_type/delta_for_debug.
pub fn stream_json_parse_line(line: &str) -> StreamParseResult {
    let empty = StreamParseResult {
        progress: None,
        text_delta: None,
        mark: None,
        block_type: None,
        delta_for_debug: None,
    };
    let v: serde_json::Value = match serde_json::from_str(line) {
        Ok(x) => x,
        Err(_) => return empty,
    };
    let typ = match v.get("type").and_then(|t| t.as_str()) {
        Some(t) => t,
        None => return empty,
    };
    if typ != "stream_event" {
        return empty;
    }
    let event = match v.get("event") {
        Some(e) => e,
        None => return empty,
    };
    let event_type = event.get("type").and_then(|t| t.as_str());

    if event_type == Some("content_block_stop") {
        return StreamParseResult {
            mark: Some(StreamEventMark::ContentBlockStop),
            ..empty
        };
    }
    if event_type == Some("message_stop") {
        return StreamParseResult {
            mark: Some(StreamEventMark::MessageStop),
            ..empty
        };
    }

    if event_type == Some("content_block_start") {
        if let Some(block) = event.get("content_block") {
            let block_type = block.get("type").and_then(|t| t.as_str());
            let block_type_str = block_type.map(|s| {
                if s == "tool_use" {
                    let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("tool");
                    format!("tool_use:{}", name)
                } else {
                    s.to_string()
                }
            });
            if block_type == Some("thinking") {
                return StreamParseResult {
                    progress: Some(ClaudeProgress::Thinking),
                    block_type: block_type_str,
                    ..empty
                };
            }
            if block_type == Some("tool_use") {
                let name = block
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("tool")
                    .to_string();
                return StreamParseResult {
                    progress: Some(ClaudeProgress::ToolUse { name }),
                    block_type: block_type_str,
                    ..empty
                };
            }
            if block_type == Some("text") {
                return StreamParseResult {
                    block_type: Some("text".into()),
                    ..empty
                };
            }
        }
        return empty;
    }

    if event_type == Some("content_block_delta") {
        if let Some(delta) = event.get("delta") {
            let delta_type = delta.get("type").and_then(|t| t.as_str());
            if delta_type == Some("text_delta") {
                let text = delta.get("text").and_then(|t| t.as_str()).map(String::from);
                return StreamParseResult {
                    text_delta: text.clone(),
                    delta_for_debug: text,
                    ..empty
                };
            }
            if delta_type == Some("input_json_delta") {
                let partial = delta.get("partial_json").and_then(|j| j.as_str()).map(String::from);
                return StreamParseResult {
                    delta_for_debug: partial,
                    ..empty
                };
            }
            if delta_type == Some("thinking_delta") {
                let partial = delta.get("partial_json").and_then(|j| j.as_str()).map(String::from);
                return StreamParseResult {
                    progress: Some(ClaudeProgress::Thinking),
                    delta_for_debug: partial,
                    ..empty
                };
            }
        }
    }

    empty
}

/// Stop marks: end of a content block or end of message. Used to flush text buffer as one TextPart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StreamEventMark {
    ContentBlockStop,
    MessageStop,
}

/// Parse a single line of stream-json output from Claude CLI; return text_delta if present.
pub fn stream_json_text_delta(line: &str) -> Option<String> {
    stream_json_parse_line(line).text_delta
}

/// Run headless `claude -p "..."` and return the full reply as a single string.
/// If `cwd` is Some, Claude runs in that directory (e.g. job workspace); otherwise uses default chat_working_dir().
pub async fn run_claude_prompt_to_string(
    prompt: &str,
    cwd: Option<std::path::PathBuf>,
) -> Result<String, String> {
    run_claude_prompt_to_string_with_progress(prompt, |_| {}, cwd).await
}

/// Like `run_claude_prompt_to_string`, but calls `on_progress(ClaudeProgress)` for thinking/tool_use
/// so the caller can show "Thinking...", "Using tool: X" (e.g. in Telegram). Callback is sync; use
/// `try_send` if sending to a channel to avoid blocking.
/// If `cwd` is Some, Claude runs in that directory (e.g. job workspace); otherwise uses chat_working_dir().
pub async fn run_claude_prompt_to_string_with_progress<F>(
    prompt: &str,
    mut on_progress: F,
    cwd: Option<std::path::PathBuf>,
) -> Result<String, String>
where
    F: FnMut(ClaudeProgress),
{
    let cwd = cwd.unwrap_or_else(chat_working_dir);
    let mut child = TokioCommand::new("claude")
        .args([
            "-p",
            prompt,
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
            "--allowedTools",
            "Read,Edit,Bash",
            "--dangerously-skip-permissions",
        ])
        .current_dir(&cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to run claude: {}", e))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "claude stdout not captured".to_string())?;

    let mut out = String::new();
    let mut reader = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = reader.next_line().await {
        let r = stream_json_parse_line(&line);
        if let Some(p) = r.progress {
            on_progress(p);
        }
        if let Some(t) = r.text_delta {
            out.push_str(&t);
        }
    }
    let _ = child.wait().await;
    Ok(out)
}

/// Run headless Claude and invoke `on_segment(ClaudeSegment)` for each progress label or text block.
/// Text blocks are flushed on content_block_stop and message_stop (organized by Claude type/stop).
/// Use for channels that send one message per segment (e.g. Feishu) with a response FIFO.
/// If `cwd` is Some, Claude runs in that directory (e.g. job workspace); otherwise uses chat_working_dir().
/// `session` controls session creation/resume; None = no session management.
/// Returns the session_id parsed from the stream-json init event, if any.
pub async fn run_claude_prompt_to_stream_parts<F>(
    prompt: &str,
    mut on_segment: F,
    cwd: Option<std::path::PathBuf>,
    session: Option<headless::SessionMode>,
) -> Result<headless::RunnerResult, String>
where
    F: FnMut(ClaudeSegment),
{
    let cwd = cwd.unwrap_or_else(chat_working_dir);
    let mut args = vec![
        "-p".to_string(),
        prompt.to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--verbose".to_string(),
        "--include-partial-messages".to_string(),
        "--allowedTools".to_string(),
        "Read,Edit,Bash".to_string(),
        "--dangerously-skip-permissions".to_string(),
    ];
    // Session management flags
    if let Some(ref sm) = session {
        match sm {
            headless::SessionMode::New(id) => {
                args.push("--session-id".to_string());
                args.push(id.clone());
            }
            headless::SessionMode::Resume(id) => {
                args.push("--resume".to_string());
                args.push(id.clone());
            }
        }
    }
    let mut child = TokioCommand::new("claude")
        .args(&args)
        .current_dir(&cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to run claude: {}", e))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "claude stdout not captured".to_string())?;

    let mut text_buffer = String::new();
    let mut in_text_block = false;
    let mut parsed_session_id: Option<String> = None;
    let mut reader = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = reader.next_line().await {
        // Try to extract session_id from the init event (first line)
        if parsed_session_id.is_none() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                if v.get("type").and_then(|t| t.as_str()) == Some("system")
                    && v.get("subtype").and_then(|t| t.as_str()) == Some("init")
                {
                    parsed_session_id = v.get("session_id").and_then(|s| s.as_str()).map(String::from);
                }
            }
        }
        let r = stream_json_parse_line(&line);
        if let Some(p) = r.progress {
            on_segment(ClaudeSegment::Progress(p));
        }
        if let Some(t) = r.text_delta {
            in_text_block = true;
            text_buffer.push_str(&t);
        }
        match r.mark {
            Some(StreamEventMark::ContentBlockStop) => {
                if in_text_block && !text_buffer.is_empty() {
                    let part = std::mem::take(&mut text_buffer);
                    on_segment(ClaudeSegment::TextPart(part));
                }
                in_text_block = false;
            }
            Some(StreamEventMark::MessageStop) => {
                if in_text_block && !text_buffer.is_empty() {
                    let part = std::mem::take(&mut text_buffer);
                    on_segment(ClaudeSegment::TextPart(part));
                }
                in_text_block = false;
            }
            None => {}
        }
    }
    let _ = child.wait().await;
    Ok(headless::RunnerResult { session_id: parsed_session_id })
}

/// Claude runner instance. Implements HeadlessRunner for unified management and dispatch.
#[derive(Debug, Default)]
pub struct ClaudeRunner;

#[async_trait::async_trait]
impl headless::HeadlessRunner for ClaudeRunner {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn supports_native_resume(&self) -> bool {
        true
    }

    async fn run_to_stream(
        &self,
        prompt: &str,
        cwd: Option<std::path::PathBuf>,
        session: Option<headless::SessionMode>,
        on_segment: &mut (dyn FnMut(headless::RunnerSegment) + Send),
    ) -> Result<headless::RunnerResult, headless::RunnerError> {
        run_claude_prompt_to_stream_parts(prompt, |seg| on_segment(seg), cwd, session).await
    }

    async fn classify_intent(
        &self,
        context: &headless::ClassifyContext,
        cwd: Option<std::path::PathBuf>,
    ) -> Result<headless::IntentResult, headless::RunnerError> {
        let cwd = cwd.unwrap_or_else(chat_working_dir);

        // Build the classification prompt
        let projects_desc = if context.projects.is_empty() {
            "No existing projects.".to_string()
        } else {
            context.projects.iter()
                .map(|p| format!("- id: {}, name: {}", p.project_id, p.name))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let current_session_desc = match &context.current_session {
            Some(info) => format!(
                "Currently active session in project '{}'. Recent context: {}",
                info.project_name, info.recent_summary
            ),
            None => "No active session.".to_string(),
        };

        let classify_prompt = format!(
            r#"You are a routing assistant. Given a user message and context, decide the action.

User message: "{}"

Current state: {}

Existing projects:
{}

Respond with ONLY a JSON object (no markdown, no explanation):
{{
  "action": "continue" | "switch_to_existing" | "new_project" | "chat_only",
  "project_id": "<id if action is switch_to_existing, else empty string>",
  "suggested_name": "<name if action is new_project, else empty string>",
  "reason": "<brief explanation in the user's language>"
}}

Rules:
- "continue": the message is about the current session's topic (continue in same project)
- "switch_to_existing": the message is about a different existing project
- "new_project": the message starts a new topic not matching any existing project; need a new project folder
- "chat_only": simple chat or question (greetings, who are you, small talk, one-off questions). No project/session create or switch. Use when the user is just chatting or asking something that does not require a code workspace.
- If no active session and no matching project, use "new_project" only when the user clearly wants to start a coding task; otherwise use "chat_only""#,
            context.user_prompt, current_session_desc, projects_desc
        );

        eprintln!("[VibeAround][im][claude] classify_intent spawning claude subprocess...");
        let mut child = TokioCommand::new("claude")
            .args(["-p", &classify_prompt, "--output-format", "json"])
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("classify_intent spawn: {}", e))?;

        let stdout = child.stdout.take().ok_or("classify_intent: no stdout")?;
        let mut output = String::new();
        let mut reader = tokio::io::BufReader::new(stdout);
        tokio::io::AsyncReadExt::read_to_string(&mut reader, &mut output)
            .await
            .map_err(|e| format!("classify_intent read: {}", e))?;
        let _ = child.wait().await;

        // Log raw output to verify if Claude actually returned (empty = likely not ran or failed)
        let raw = output.trim();
        let preview = if raw.len() > 400 { &raw[..400] } else { raw };
        eprintln!(
            "[VibeAround][im][claude] classify_intent raw_output_len={} preview=\"{}\"",
            raw.len(),
            preview.replace('\n', " ")
        );
        if raw.is_empty() {
            return Err("classify_intent: subprocess returned empty output".into());
        }

        // Parse the JSON response (outer wrapper may have "result" as string containing inner JSON)
        let outer: serde_json::Value = serde_json::from_str(raw)
            .map_err(|e| format!("classify_intent parse outer: {} output={}", e, raw))?;
        let v: serde_json::Value = match outer.get("result") {
            Some(serde_json::Value::String(s)) => serde_json::from_str(s)
                .map_err(|e| format!("classify_intent parse inner result: {} result_str={}", e, s))?,
            Some(other) => other.clone(),
            None => outer,
        };

        let action = v.get("action").and_then(|a| a.as_str()).unwrap_or("new_project");
        let reason = v.get("reason").and_then(|r| r.as_str()).unwrap_or("").to_string();

        match action {
            "continue" => Ok(headless::IntentResult::ContinueCurrent { reason }),
            "switch_to_existing" => {
                let project_id = v.get("project_id").and_then(|p| p.as_str()).unwrap_or("").to_string();
                Ok(headless::IntentResult::ExistingProject { project_id, reason })
            }
            "chat_only" => Ok(headless::IntentResult::ChatOnly { reason }),
            _ => {
                let name = v.get("suggested_name").and_then(|n| n.as_str()).unwrap_or("untitled").to_string();
                Ok(headless::IntentResult::NewProject { suggested_name: name, reason })
            }
        }
    }
}
