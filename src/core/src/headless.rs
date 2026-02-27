//! Headless CLI runner: run a prompt through a CLI tool and return unified text output.
//! No IM or HTTP; used by server (WebSocket chat, IM worker).
//! Each runner (Claude, etc.) implements HeadlessRunner so we can manage and dispatch uniformly.

use std::path::PathBuf;

use async_trait::async_trait;

pub mod context;
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

// -- Session and intent types used by HeadlessRunner --

/// How to handle session when invoking a runner.
#[derive(Debug, Clone)]
pub enum SessionMode {
    /// Start a new session with a pre-generated ID (e.g. `--session-id <uuid>`).
    New(String),
    /// Resume an existing session (e.g. `--resume <session_id>`).
    Resume(String),
}

/// Result returned by `run_to_stream` — carries runner-specific session info.
#[derive(Debug, Clone, Default)]
pub struct RunnerResult {
    /// The session ID reported by the runner (parsed from output), if any.
    pub session_id: Option<String>,
}

/// Info about an existing project, passed to `classify_intent`.
#[derive(Debug, Clone)]
pub struct ProjectInfo {
    pub project_id: String,
    pub name: String,
    pub path: String,
}

/// Info about the currently active session, passed to `classify_intent`.
#[derive(Debug, Clone)]
pub struct CurrentSessionInfo {
    pub session_id: String,
    pub project_name: String,
    pub recent_summary: String,
}

/// Context for intent classification.
#[derive(Debug, Clone)]
pub struct ClassifyContext {
    pub user_prompt: String,
    pub projects: Vec<ProjectInfo>,
    pub current_session: Option<CurrentSessionInfo>,
}

/// Result of intent classification — each variant carries a reason for the user.
#[derive(Debug, Clone)]
pub enum IntentResult {
    /// Continue the current session.
    ContinueCurrent { reason: String },
    /// Switch to an existing project.
    ExistingProject { project_id: String, reason: String },
    /// Create a new project.
    NewProject { suggested_name: String, reason: String },
}

/// Unified JSON wire format for streaming headless output to any consumer (WS chat, IM, etc.).
/// Both IM worker and WebSocket chat handler use these to produce consistent messages.
pub mod wire {
    use super::{ClaudeProgress, ClaudeSegment};

    /// Format a ClaudeSegment::Progress as a JSON string.
    pub fn progress_json(p: &ClaudeProgress) -> String {
        let label = match p {
            ClaudeProgress::Thinking => "Thinking...".to_string(),
            ClaudeProgress::ToolUse { name } => format!("Using tool: {}...", name),
        };
        serde_json::json!({ "progress": label }).to_string()
    }

    /// Format a ClaudeSegment::TextPart as a JSON string.
    pub fn text_json(text: &str) -> String {
        serde_json::json!({ "text": text }).to_string()
    }

    /// Format a stream-done marker.
    pub fn done_json() -> String {
        serde_json::json!({ "done": true }).to_string()
    }

    /// Format an error message.
    pub fn error_json(msg: &str) -> String {
        serde_json::json!({ "error": msg }).to_string()
    }

    /// Format a job creation event (job_id + preview path).
    pub fn job_json(job_id: &str, preview: &str) -> String {
        serde_json::json!({ "job_id": job_id, "preview": preview }).to_string()
    }

    /// Convert a ClaudeSegment to its wire JSON string.
    pub fn segment_to_json(seg: &ClaudeSegment) -> String {
        match seg {
            ClaudeSegment::Progress(p) => progress_json(p),
            ClaudeSegment::TextPart(text) => text_json(text),
        }
    }
}

/// Unified runner trait: same fields and methods for all implementations (Claude, future plugins).
/// Enables single dispatch and config-driven runner selection.
#[async_trait]
pub trait HeadlessRunner: Send + Sync {
    /// Runner id (e.g. "claude") for config and logging.
    fn name(&self) -> &'static str;

    /// Whether this runner supports native session resume (e.g. Claude Code `--resume`).
    /// If true, ContextManager skips its own compression and lets the runner handle context.
    fn supports_native_resume(&self) -> bool { false }

    /// Run prompt and stream segments via callback. cwd = None uses default chat dir.
    /// `session` controls session creation/resume; None = no session management.
    async fn run_to_stream(
        &self,
        prompt: &str,
        cwd: Option<PathBuf>,
        session: Option<SessionMode>,
        on_segment: &mut (dyn FnMut(RunnerSegment) + Send),
    ) -> Result<RunnerResult, RunnerError>;

    /// Classify user intent: continue current session, switch to existing project, or create new.
    /// Returns structured result with a reason string (shown to user on switch/create).
    async fn classify_intent(
        &self,
        context: &ClassifyContext,
        cwd: Option<PathBuf>,
    ) -> Result<IntentResult, RunnerError>;
}
