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
    /// User message to persist (written to the Manager's session).
    UserMessage {
        chat_id: String,
        content: String,
    },
}

/// Spawn the message hub background task.
///
/// `default_sessions_dir` — fallback dir for session files (Manager sessions).
/// Returns an unbounded sender; dropping it stops the hub.
pub fn spawn_hub(default_sessions_dir: PathBuf) -> mpsc::UnboundedSender<HubMessage> {
    let (tx, mut rx) = mpsc::unbounded_channel::<HubMessage>();

    tokio::spawn(async move {
        let mut writers: HashMap<String, SessionWriter> = HashMap::new();
        // Track the first agent_id seen — that's the Manager
        let mut manager_agent_id: Option<String> = None;

        while let Some(msg) = rx.recv().await {
            match msg {
                HubMessage::Event { chat_id: _, ref agent_id, ref event } => {
                    let is_turn_complete = matches!(event, AgentEvent::TurnComplete { .. });

                    // First agent is the Manager
                    if manager_agent_id.is_none() {
                        manager_agent_id = Some(agent_id.clone());
                    }
                    let is_manager = manager_agent_id.as_deref() == Some(agent_id);

                    // Lazily open a writer for this agent
                    if !writers.contains_key(agent_id) {
                        let (kind, workspace) = parse_agent_id(agent_id);
                        let role = if is_manager { "manager" } else { "worker" };

                        // Manager → default_sessions_dir, Worker → <workspace>/.vibearound/sessions/
                        let sessions_dir = if is_manager {
                            default_sessions_dir.clone()
                        } else {
                            super::session_store::workspace_sessions_dir(std::path::Path::new(&workspace))
                        };

                        match SessionWriter::create(&sessions_dir, kind, role, &workspace, None, None) {
                            Ok(w) => {
                                writers.insert(agent_id.clone(), w);
                            }
                            Err(e) => {
                                eprintln!("[message-hub] failed to create session for {}: {}", agent_id, e);
                                continue;
                            }
                        }
                    }

                    if let Some(writer) = writers.get_mut(agent_id) {
                        writer.append_agent_event(agent_id, event);
                    }

                    // TurnComplete → close the writer (file handle released)
                    if is_turn_complete {
                        if let Some(w) = writers.remove(agent_id) {
                            eprintln!("[message-hub] closed session for {}: {}", agent_id, w.path.display());
                        }
                    }
                }
                HubMessage::UserMessage { chat_id: _, content } => {
                    // Write user message to the Manager's session
                    if let Some(ref mid) = manager_agent_id {
                        if let Some(writer) = writers.get_mut(mid) {
                            writer.append_user_message(&content);
                        }
                    }
                }
            }
        }
        eprintln!("[message-hub] stopped, {} writers remaining", writers.len());
    });

    tx
}

/// Parse agent_id format "kind:/path/to/workspace" → (AgentKind, workspace_path)
fn parse_agent_id(agent_id: &str) -> (AgentKind, String) {
    match agent_id.split_once(':') {
        Some((kind_str, workspace)) => {
            let kind = AgentKind::from_str_loose(kind_str).unwrap_or(AgentKind::Claude);
            (kind, workspace.to_string())
        }
        None => (AgentKind::Claude, agent_id.to_string()),
    }
}
