//! Context management: tracks token usage per session and triggers compression.
//! If the runner supports native resume (e.g. Claude Code --resume), compression is skipped.
//! Otherwise, a summary is generated and stored in chat_sessions.summary as a fallback.

use std::sync::{Arc, Mutex};
use rusqlite::Connection;

use crate::headless::HeadlessRunner;
use crate::im::session as session_store;

/// Default token threshold before triggering context compression.
const DEFAULT_COMPRESS_THRESHOLD: i64 = 50_000;

pub struct ContextManager {
    db: Arc<Mutex<Connection>>,
    compress_threshold: i64,
}

impl ContextManager {
    pub fn new(db: Arc<Mutex<Connection>>) -> Self {
        Self { db, compress_threshold: DEFAULT_COMPRESS_THRESHOLD }
    }

    pub fn with_threshold(db: Arc<Mutex<Connection>>, threshold: i64) -> Self {
        Self { db, compress_threshold: threshold }
    }

    /// Check if a session's total_tokens exceeds the compression threshold.
    pub fn needs_compression(&self, session_id: &str) -> bool {
        let conn = self.db.lock().unwrap();
        session_store::get_session(&conn, session_id)
            .ok()
            .flatten()
            .map(|s| s.total_tokens >= self.compress_threshold)
            .unwrap_or(false)
    }

    /// Generate a summary of the session's conversation history and store it.
    /// Uses the runner itself to produce the summary.
    /// After compression, total_tokens is reset to 0.
    pub async fn compress(&self, session_id: &str, runner: &dyn HeadlessRunner, cwd: Option<std::path::PathBuf>) {
        // Get current summary (if any) to include in the compression prompt
        let current_summary = {
            let conn = self.db.lock().unwrap();
            session_store::get_session(&conn, session_id)
                .ok()
                .flatten()
                .and_then(|s| s.summary)
                .unwrap_or_default()
        };

        let compress_prompt = format!(
            "Summarize the conversation so far in a concise paragraph. \
             Focus on key decisions, files modified, and current state. \
             Previous summary (if any): {}",
            if current_summary.is_empty() { "None" } else { &current_summary }
        );

        // Use the runner to generate the summary (reusing the same session so it has context)
        let mut summary_text = String::new();
        let _ = runner.run_to_stream(
            &compress_prompt,
            cwd,
            None, // no session mode for the compression call itself
            &mut |seg| {
                if let crate::headless::RunnerSegment::TextPart(text) = seg {
                    summary_text.push_str(&text);
                }
            },
        ).await;

        if !summary_text.is_empty() {
            let conn = self.db.lock().unwrap();
            let _ = session_store::update_summary(&conn, session_id, &summary_text, 0);
        }
    }

    /// Get the context prefix for a session.
    /// If the runner supports native resume, returns None (runner manages its own context).
    /// Otherwise, returns the stored summary as a prompt prefix.
    pub fn get_context_prefix(&self, session_id: &str, runner: &dyn HeadlessRunner) -> Option<String> {
        if runner.supports_native_resume() {
            return None;
        }
        let conn = self.db.lock().unwrap();
        session_store::get_session(&conn, session_id)
            .ok()
            .flatten()
            .and_then(|s| s.summary)
            .filter(|s| !s.is_empty())
            .map(|s| format!("[Previous conversation summary: {}]\n\n", s))
    }
}
