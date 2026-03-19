//! File-based session store: JSONL append-only logs.
//!
//! Manager sessions:  ~/.vibearound/sessions/
//! Worker sessions:   <workspace>/.vibearound/sessions/
//!
//! File naming: {date}_{agent_kind}_{cli_session_id_prefix}.jsonl
//! First line is a header with metadata, subsequent lines are events.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::agent::{AgentEvent, AgentKind};

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// Session header — first line of the JSONL file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHeader {
    /// Schema version.
    pub v: u32,
    /// Agent CLI kind.
    pub kind: String,
    /// CLI's own session ID (for --resume).
    pub cli_session_id: Option<String>,
    /// Workspace path.
    pub workspace: String,
    /// Manager or worker.
    pub role: String,
    /// Human-readable summary (first user message or LLM-generated).
    pub summary: Option<String>,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
}

/// A single event line in the JSONL file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEvent {
    /// ISO 8601 timestamp.
    pub ts: String,
    /// "user" or "assistant".
    pub role: String,
    /// Agent ID (e.g. "claude:/path/to/workspace").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Event type: text, thinking, tool_use, tool_result, turn_complete, error, progress.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    /// Main content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Extra structured data (tool name, input, etc).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Metadata returned when listing sessions (parsed from header only).
#[derive(Debug, Clone)]
pub struct SessionMeta {
    pub path: PathBuf,
    pub header: SessionHeader,
}

// ---------------------------------------------------------------------------
// SessionWriter — append-only handle
// ---------------------------------------------------------------------------

pub struct SessionWriter {
    file: File,
    pub path: PathBuf,
    pub header: SessionHeader,
}

impl SessionWriter {
    /// Create a new session file and write the header.
    pub fn create(
        sessions_dir: &Path,
        kind: AgentKind,
        role: &str,
        workspace: &str,
        cli_session_id: Option<&str>,
        summary: Option<&str>,
    ) -> std::io::Result<Self> {
        fs::create_dir_all(sessions_dir)?;

        let now = chrono::Utc::now();
        let date = now.format("%Y-%m-%d").to_string();
        let id_prefix = cli_session_id
            .map(|s| s.chars().take(8).collect::<String>())
            .unwrap_or_else(|| uuid_short());

        let filename = format!("{}_{}_{}_{}.jsonl", date, kind, role, id_prefix);
        let path = sessions_dir.join(&filename);

        let header = SessionHeader {
            v: 1,
            kind: kind.to_string(),
            cli_session_id: cli_session_id.map(String::from),
            workspace: workspace.to_string(),
            role: role.to_string(),
            summary: summary.map(String::from),
            created_at: now.to_rfc3339(),
        };

        let mut file = File::create(&path)?;
        let header_json = serde_json::to_string(&header).unwrap();
        writeln!(file, "{}", header_json)?;
        file.flush()?;

        eprintln!("[session] created {}", path.display());
        Ok(Self { file, path, header })
    }

    /// Append a user message.
    pub fn append_user_message(&mut self, content: &str) {
        let event = SessionEvent {
            ts: chrono::Utc::now().to_rfc3339(),
            role: "user".to_string(),
            agent_id: None,
            event: None,
            content: Some(content.to_string()),
            data: None,
        };
        self.append_line(&event);
    }

    /// Append an agent event.
    pub fn append_agent_event(&mut self, agent_id: &str, event: &AgentEvent) {
        let (event_type, content, data) = agent_event_to_parts(event);
        let se = SessionEvent {
            ts: chrono::Utc::now().to_rfc3339(),
            role: "assistant".to_string(),
            agent_id: Some(agent_id.to_string()),
            event: Some(event_type.to_string()),
            content,
            data,
        };
        self.append_line(&se);
    }

    /// Update the CLI session ID (called after agent init when CLI reports its session).
    pub fn update_cli_session_id(&mut self, cli_session_id: &str) {
        self.header.cli_session_id = Some(cli_session_id.to_string());
        // Rewrite is not worth it for append-only; just log it as a meta event
        let se = SessionEvent {
            ts: chrono::Utc::now().to_rfc3339(),
            role: "system".to_string(),
            agent_id: None,
            event: Some("session_id_update".to_string()),
            content: Some(cli_session_id.to_string()),
            data: None,
        };
        self.append_line(&se);
    }

    /// Update summary.
    pub fn update_summary(&mut self, summary: &str) {
        self.header.summary = Some(summary.to_string());
        let se = SessionEvent {
            ts: chrono::Utc::now().to_rfc3339(),
            role: "system".to_string(),
            agent_id: None,
            event: Some("summary_update".to_string()),
            content: Some(summary.to_string()),
            data: None,
        };
        self.append_line(&se);
    }

    fn append_line(&mut self, event: &SessionEvent) {
        if let Ok(json) = serde_json::to_string(event) {
            if let Err(e) = writeln!(self.file, "{}", json) {
                eprintln!("[session] write error: {}", e);
            }
            let _ = self.file.flush();
        }
    }
}

// ---------------------------------------------------------------------------
// Read / list
// ---------------------------------------------------------------------------

/// List all sessions in a directory, sorted by creation date (newest first).
pub fn list_sessions(sessions_dir: &Path) -> Vec<SessionMeta> {
    let mut sessions = Vec::new();
    let entries = match fs::read_dir(sessions_dir) {
        Ok(e) => e,
        Err(_) => return sessions,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        if let Some(header) = read_header(&path) {
            sessions.push(SessionMeta { path, header });
        }
    }
    sessions.sort_by(|a, b| b.header.created_at.cmp(&a.header.created_at));
    sessions
}

/// Find the latest session for a given agent kind in a directory.
pub fn latest_session(sessions_dir: &Path, kind: AgentKind) -> Option<SessionMeta> {
    let kind_str = kind.to_string();
    list_sessions(sessions_dir)
        .into_iter()
        .find(|s| s.header.kind == kind_str)
}

/// Read the header (first line) of a session file.
pub fn read_header(path: &Path) -> Option<SessionHeader> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);
    let first_line = reader.lines().next()?.ok()?;
    serde_json::from_str(&first_line).ok()
}

/// Read all events (excluding header) from a session file.
pub fn read_events(path: &Path) -> Vec<SessionEvent> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let reader = BufReader::new(file);
    reader
        .lines()
        .skip(1) // skip header
        .filter_map(|line| line.ok())
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect()
}

/// Get the CLI session ID from the latest session_id_update event, or from header.
pub fn get_cli_session_id(path: &Path) -> Option<String> {
    // Check events in reverse for session_id_update
    let events = read_events(path);
    for event in events.iter().rev() {
        if event.event.as_deref() == Some("session_id_update") {
            return event.content.clone();
        }
    }
    // Fall back to header
    read_header(path).and_then(|h| h.cli_session_id)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Sessions directory for Manager agent.
pub fn manager_sessions_dir() -> PathBuf {
    crate::config::data_dir().join("sessions")
}

/// Sessions directory for a Worker agent in a workspace.
pub fn workspace_sessions_dir(workspace: &Path) -> PathBuf {
    workspace.join(".vibearound").join("sessions")
}

fn agent_event_to_parts(event: &AgentEvent) -> (&'static str, Option<String>, Option<serde_json::Value>) {
    match event {
        AgentEvent::Text(t) => ("text", Some(t.clone()), None),
        AgentEvent::Thinking(t) => ("thinking", Some(t.clone()), None),
        AgentEvent::Progress(s) => ("progress", Some(s.clone()), None),
        AgentEvent::ToolUse { name, id, input } => (
            "tool_use",
            None,
            Some(serde_json::json!({ "name": name, "id": id, "input": input })),
        ),
        AgentEvent::ToolResult { id, output, is_error } => (
            "tool_result",
            None,
            Some(serde_json::json!({ "id": id, "output": output, "is_error": is_error })),
        ),
        AgentEvent::TurnComplete { session_id, cost_usd } => (
            "turn_complete",
            None,
            Some(serde_json::json!({ "session_id": session_id, "cost_usd": cost_usd })),
        ),
        AgentEvent::Error(e) => ("error", Some(e.clone()), None),
    }
}

fn uuid_short() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
    format!("{:x}", t & 0xFFFFFFFF)
}
