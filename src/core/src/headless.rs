//! Headless CLI runner: run a prompt through a CLI tool and return unified text output.
//! No IM or HTTP; used by server (WebSocket chat, IM worker).
//! Each supported tool lives in headless::runners (e.g. runners::claude).

pub mod runners;

/// Default working directory for headless chat (no PTY). Override via config later.
pub fn chat_working_dir() -> std::path::PathBuf {
    let home = std::env::var("HOME")
        .unwrap_or_else(|_| std::env::var("USERPROFILE").unwrap_or_else(|_| "/tmp".into()));
    std::path::PathBuf::from(home).join("test")
}

// Re-export so callers can use headless::run_claude_prompt_to_string etc.
pub use runners::claude::{
    run_claude_prompt_to_string, run_claude_prompt_to_string_with_progress,
    run_claude_prompt_to_stream_parts, stream_json_text_delta, ClaudeProgress, ClaudeSegment,
    StreamEventMark,
};
