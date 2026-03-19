//! Event log: persist all agent events to JSONL session files.
//!
//! Every AgentEvent flowing through the IM worker is logged here, tagged with
//! the originating agent_id so Manager vs Worker events can be distinguished.

use tokio::sync::mpsc;

use crate::agent::AgentEvent;

/// A tagged event ready for logging and display.
#[derive(Debug, Clone)]
pub struct TaggedEvent {
    pub chat_id: String,
    pub agent_id: String,
    pub event: AgentEvent,
}

/// Spawn a background task that writes events to a JSONL session file.
///
/// Returns an unbounded sender; dropping it stops the logger.
pub fn spawn_logger(
    session_writer: super::session_store::SessionWriter,
) -> mpsc::UnboundedSender<TaggedEvent> {
    let (tx, mut rx) = mpsc::unbounded_channel::<TaggedEvent>();

    tokio::spawn(async move {
        let mut writer = session_writer;
        while let Some(tagged) = rx.recv().await {
            writer.append_agent_event(&tagged.agent_id, &tagged.event);
        }
        eprintln!("[event-log] logger stopped, session file: {}", writer.path.display());
    });

    tx
}
