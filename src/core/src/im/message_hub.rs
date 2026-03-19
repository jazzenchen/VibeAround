//! Message hub: receives all agent events and persists them to JSONL session files.
//!
//! Routes events by agent_id to per-agent SessionWriters.
//! Writers are opened lazily on first event and closed on TurnComplete.
//! Future: can also broadcast to WebSocket, push notifications, etc.

use std::collections::HashMap;
use std::path::PathBuf;

use tokio::sync::mpsc;

use crate::agent::{AgentEvent, AgentKind};
use super::session_store::SessionWriter;

/// Messages sent to the hub.
#[derive(Debug, Clone)]
pub enum HubMessage {
    /// An agent event to persist.
    Event {
        chat_id: String,
        agent_id: String,
        event: AgentEvent,
    },
    /// User message to persist.
    UserMessage {
        chat_id: String,
        content: String,
    },
}

/// Spawn the message hub background task.
///
/// `sessions_dir` — where to write JSONL files.
/// Returns an unbounded sender; dropping it stops the hub.
pub fn spawn_hub(sessions_dir: PathBuf) -> mpsc::UnboundedSender<HubMessage> {
    let (tx, mut rx) = mpsc::unbounded_channel::<HubMessage>();

    tokio::spawn(async move {
        let mut writers: HashMap<String, SessionWriter> = HashMap::new();

        while let Some(msg) = rx.recv().await {
            match msg {
                HubMessage::Event { chat_id: _, agent_id, event } => {
                    // On TurnComplete, write the event then close the writer
                    let is_turn_complete = matches!(event, AgentEvent::TurnComplete { .. });

                    // Lazily open a writer for this agent
                    let writer = writers.entry(agent_id.clone()).or_insert_with(|| {
                        let kind_str = agent_id.split_once(':').map(|(k, _)| k).unwrap_or("unknown");
                        let kind = AgentKind::from_str_loose(kind_str).unwrap_or(AgentKind::Claude);
                        let role = if agent_id.contains("manager") { "manager" } else { "worker" };
                        match SessionWriter::create(&sessions_dir, kind, role, &agent_id, None, None) {
                            Ok(w) => w,
                            Err(e) => {
                                eprintln!("[message-hub] failed to create session file: {}", e);
                                // Create a dummy that writes to /dev/null — shouldn't happen
                                panic!("session writer creation failed: {}", e);
                            }
                        }
                    });

                    writer.append_agent_event(&agent_id, &event);

                    if is_turn_complete {
                        // Remove writer → Drop → close file handle
                        let w = writers.remove(&agent_id);
                        if let Some(w) = w {
                            eprintln!("[message-hub] closed session file for {}: {}", agent_id, w.path.display());
                        }
                    }
                }
                HubMessage::UserMessage { chat_id: _, content } => {
                    // Write user message to all open writers (or the most recent one)
                    for writer in writers.values_mut() {
                        writer.append_user_message(&content);
                    }
                    // If no writers open yet, we'll catch it when the agent responds
                    if writers.is_empty() {
                        eprintln!("[message-hub] user message received but no writers open yet");
                    }
                }
            }
        }

        // Hub shutting down — drop all writers
        let count = writers.len();
        writers.clear();
        eprintln!("[message-hub] stopped, closed {} session files", count);
    });

    tx
}
