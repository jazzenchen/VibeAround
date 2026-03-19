//! Event log: persist all agent events per chat_id for full audit trail.
//!
//! Every AgentEvent flowing through the IM worker is logged here, tagged with
//! the originating agent_id so Manager vs Worker events can be distinguished.

use rusqlite::Connection;
use tokio::sync::mpsc;

use crate::agent::AgentEvent;

/// A tagged event ready for logging and display.
#[derive(Debug, Clone)]
pub struct TaggedEvent {
    pub chat_id: String,
    pub agent_id: String,
    pub event: AgentEvent,
}

/// Insert a single event into the event_log table.
pub fn insert_event(conn: &Connection, chat_id: &str, agent_id: &str, event: &AgentEvent) {
    let (event_type, content) = event_to_row(event);
    let now = chrono::Utc::now().to_rfc3339();
    if let Err(e) = conn.execute(
        "INSERT INTO event_log (chat_id, agent_id, event_type, content, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![chat_id, agent_id, event_type, content, now],
    ) {
        eprintln!("[event_log] insert error: {}", e);
    }
}

/// Spawn a background task that drains tagged events and writes them to sqlite.
/// Returns the sender half — callers push `TaggedEvent`s into it.
pub fn spawn_logger(db_path: std::path::PathBuf) -> mpsc::UnboundedSender<TaggedEvent> {
    let (tx, mut rx) = mpsc::unbounded_channel::<TaggedEvent>();

    std::thread::Builder::new()
        .name("event-logger".into())
        .spawn(move || {
            let conn = match rusqlite::Connection::open(&db_path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("[event_log] failed to open db: {}", e);
                    return;
                }
            };
            let _ = conn.pragma_update(None, "journal_mode", "WAL");

            // Block on the channel in a dedicated thread
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                while let Some(tagged) = rx.recv().await {
                    insert_event(&conn, &tagged.chat_id, &tagged.agent_id, &tagged.event);
                }
            });
        })
        .expect("Failed to spawn event-logger thread");

    tx
}

fn event_to_row(event: &AgentEvent) -> (&'static str, Option<String>) {
    match event {
        AgentEvent::Text(t) => ("text", Some(t.clone())),
        AgentEvent::Thinking(t) => ("thinking", Some(t.clone())),
        AgentEvent::Progress(s) => ("progress", Some(s.clone())),
        AgentEvent::ToolUse { name, id, input } => (
            "tool_use",
            Some(serde_json::json!({ "name": name, "id": id, "input": input }).to_string()),
        ),
        AgentEvent::ToolResult { id, output, is_error } => (
            "tool_result",
            Some(serde_json::json!({ "id": id, "output": output, "is_error": is_error }).to_string()),
        ),
        AgentEvent::TurnComplete { session_id, cost_usd } => (
            "turn_complete",
            Some(serde_json::json!({ "session_id": session_id, "cost_usd": cost_usd }).to_string()),
        ),
        AgentEvent::Error(e) => ("error", Some(e.clone())),
    }
}
