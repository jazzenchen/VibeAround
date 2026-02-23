//! Headless CLI runner: run a prompt through a CLI tool and return unified text output.
//! No IM or HTTP; used by server (WebSocket chat, IM worker).
//! Each runner (Claude, etc.) implements HeadlessRunner so we can manage and dispatch uniformly.

use std::path::PathBuf;

use async_trait::async_trait;

pub mod runners;

/// Default working directory for headless chat (no PTY). Override via config later.
pub fn chat_working_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .unwrap_or_else(|_| std::env::var("USERPROFILE").unwrap_or_else(|_| "/tmp".into()));
    PathBuf::from(home).join("test")
}

// Re-export segment types as the unified runner output (same shape for all runners).
pub use runners::claude::{ClaudeProgress as RunnerProgress, ClaudeSegment as RunnerSegment};

// Re-export so callers can use headless::run_claude_prompt_to_string etc.
pub use runners::claude::{
    run_claude_prompt_to_string, run_claude_prompt_to_string_with_progress,
    run_claude_prompt_to_stream_parts, stream_json_text_delta, ClaudeProgress, ClaudeRunner,
    ClaudeSegment, StreamEventMark,
};

/// Error from running a headless prompt (spawn failure, stream parse, etc.).
pub type RunnerError = String;

/// Unified runner trait: same fields and methods for all implementations (Claude, future plugins).
/// Enables single dispatch and config-driven runner selection.
#[async_trait]
pub trait HeadlessRunner: Send + Sync {
    /// Runner id (e.g. "claude") for config and logging.
    fn name(&self) -> &'static str;

    /// Run prompt and stream segments via callback. cwd = None uses default chat dir.
    async fn run_to_stream(
        &self,
        prompt: &str,
        cwd: Option<PathBuf>,
        on_segment: &mut (dyn FnMut(RunnerSegment) + Send),
    ) -> Result<(), RunnerError>;
}
