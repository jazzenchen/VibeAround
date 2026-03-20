//! Unified agent backend: all agents communicate via ACP (Agent Client Protocol).
//!
//! - Gemini: speaks ACP natively (`gemini --experimental-acp`)
//! - Claude: wrapped via `claude_acp` adapter (Claude SDK protocol → ACP translation)
//!
//! The worker only sees `AgentBackend` + `AgentEvent`, backed by ACP `ClientSideConnection`.

pub mod claude_acp;
pub mod claude_sdk;
pub mod codex_acp;
pub mod codex_jsonl;
pub mod gemini_acp;
pub mod manager_prompt;
pub mod opencode_acp;
pub mod opencode_jsonl;

use std::fmt;
use std::path::Path;
use std::sync::Arc;

/// Which agent CLI to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Claude,
    Gemini,
    OpenCode,
    Codex,
}

impl fmt::Display for AgentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentKind::Claude => write!(f, "claude"),
            AgentKind::Gemini => write!(f, "gemini"),
            AgentKind::OpenCode => write!(f, "opencode"),
            AgentKind::Codex => write!(f, "codex"),
        }
    }
}

impl AgentKind {
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "claude" | "claude-code" => Some(Self::Claude),
            "gemini" | "gemini-cli" => Some(Self::Gemini),
            "opencode" | "open-code" => Some(Self::OpenCode),
            "codex" | "openai-codex" => Some(Self::Codex),
            _ => None,
        }
    }

    /// All available agent kinds.
    pub fn all() -> &'static [AgentKind] {
        &[AgentKind::Claude, AgentKind::Gemini, AgentKind::OpenCode, AgentKind::Codex]
    }

    /// Only agents enabled in config (settings.json `enabled_agents`).
    /// Falls back to all agents if not configured.
    pub fn enabled() -> Vec<AgentKind> {
        crate::config::ensure_loaded().enabled_agents.clone()
    }

    /// Whether this agent kind is enabled in config.
    pub fn is_enabled(&self) -> bool {
        crate::config::ensure_loaded().enabled_agents.contains(self)
    }

    /// Emoji icon for this agent.

    /// Short description of this agent.
    pub fn description(&self) -> &'static str {
        match self {
            AgentKind::Claude => "Anthropic Claude Code",
            AgentKind::Gemini => "Google Gemini CLI",
            AgentKind::OpenCode => "OpenCode AI Agent",
            AgentKind::Codex => "OpenAI Codex CLI",
        }
    }
}

/// Events emitted by an agent backend, consumed by the IM worker.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// Assistant text chunk (streaming).
    Text(String),
    /// Thinking / reasoning text chunk.
    Thinking(String),
    /// Progress indicator: "Thinking...", "Using tool: Bash", etc.
    Progress(String),
    /// A tool call started (with optional input JSON).
    ToolUse { name: String, id: String, input: Option<String> },
    /// A tool call produced a result.
    ToolResult { id: String, output: Option<String>, is_error: bool },
    /// The agent's turn is complete.
    TurnComplete {
        session_id: Option<String>,
        cost_usd: Option<f64>,
    },
    /// An error occurred.
    Error(String),
}

/// Unified interface for agent backends (Claude, Gemini, etc.).
/// Both backends are backed by ACP `ClientSideConnection` under the hood.
#[async_trait::async_trait]
pub trait AgentBackend: Send + Sync {
    /// Spawn the agent subprocess, establish ACP connection, create session.
    /// `system_prompt` is injected for Manager agents only (None for Workers).
    async fn start(&mut self, cwd: &Path, system_prompt: Option<&str>) -> Result<(), String>;

    /// Send a user message via ACP `prompt()`. Blocks until the turn completes.
    /// Events are delivered via `subscribe()`.
    async fn send_message(&self, text: &str) -> Result<(), String>;

    /// Fire a prompt without waiting for the turn to complete.
    /// Returns immediately after the command is queued.
    /// Use `subscribe()` to consume events; the turn ends with `AgentEvent::TurnComplete`.
    async fn send_message_fire(&self, text: &str) -> Result<(), String>;

    /// Subscribe to the agent's event stream.
    fn subscribe(&self) -> tokio::sync::broadcast::Receiver<AgentEvent>;

    /// Gracefully shut down the agent subprocess.
    async fn shutdown(&mut self);

    /// Which agent kind this backend represents.
    fn kind(&self) -> AgentKind;
}

/// Create a new (unstarted) agent backend for the given kind.
/// All four agents now use the unified ACP backend:
/// - Claude: in-process ACP bridge via claude_sdk
/// - Gemini: `gemini --experimental-acp` subprocess
/// - OpenCode: `opencode acp` subprocess (native ACP over stdio)
/// - Codex: `codex-acp` subprocess (ACP bridge from cola-io/codex-acp)
pub fn create_backend(kind: AgentKind) -> Box<dyn AgentBackend> {
    Box::new(AcpBackend::new(kind))
}

// ---------------------------------------------------------------------------
// Unified ACP backend — wraps ClientSideConnection for both Claude and Gemini
// ---------------------------------------------------------------------------

use std::path::PathBuf;
use tokio::sync::{broadcast, mpsc, oneshot};

/// Commands sent from the main (Send) world to the ACP thread.
enum AcpCmd {
    Prompt {
        text: String,
        done_tx: oneshot::Sender<Result<(), String>>,
    },
    Shutdown,
}

/// A single ACP-backed agent backend. Works for both Claude and Gemini.
/// Runs the ACP event loop on a dedicated thread (ACP futures are `!Send`).
pub struct AcpBackend {
    agent_kind: AgentKind,
    event_tx: broadcast::Sender<AgentEvent>,
    cmd_tx: Option<mpsc::Sender<AcpCmd>>,
    thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl AcpBackend {
    pub fn new(agent_kind: AgentKind) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        Self {
            agent_kind,
            event_tx,
            cmd_tx: None,
            thread_handle: None,
        }
    }
}

#[async_trait::async_trait]
impl AgentBackend for AcpBackend {
    async fn start(&mut self, cwd: &Path, system_prompt: Option<&str>) -> Result<(), String> {
        let cwd = cwd.to_path_buf();
        let event_tx = self.event_tx.clone();
        let agent_kind = self.agent_kind;
        let system_prompt_owned = system_prompt.map(|s| s.to_string());
        let (cmd_tx, cmd_rx) = mpsc::channel::<AcpCmd>(32);
        let (ready_tx, ready_rx) = oneshot::channel::<Result<(), String>>();

        let handle = std::thread::Builder::new()
            .name(format!("{}-acp", agent_kind))
            .spawn(move || {
                run_acp_thread(agent_kind, cwd, event_tx, cmd_rx, ready_tx, system_prompt_owned);
            })
            .map_err(|e| format!("Failed to spawn ACP thread: {}", e))?;

        self.cmd_tx = Some(cmd_tx);
        self.thread_handle = Some(handle);

        ready_rx
            .await
            .map_err(|_| "ACP thread died during init".to_string())?
    }

    async fn send_message(&self, text: &str) -> Result<(), String> {
        let cmd_tx = self.cmd_tx.as_ref().ok_or("Agent not started")?;
        let (done_tx, done_rx) = oneshot::channel();
        cmd_tx
            .send(AcpCmd::Prompt {
                text: text.to_string(),
                done_tx,
            })
            .await
            .map_err(|_| "ACP thread gone".to_string())?;
        done_rx.await.map_err(|_| "ACP thread gone".to_string())?
    }

    async fn send_message_fire(&self, text: &str) -> Result<(), String> {
        let cmd_tx = self.cmd_tx.as_ref().ok_or("Agent not started")?;
        let (done_tx, _done_rx) = oneshot::channel();
        cmd_tx
            .send(AcpCmd::Prompt {
                text: text.to_string(),
                done_tx,
            })
            .await
            .map_err(|_| "ACP thread gone".to_string())?;
        // Return immediately — caller uses event stream to detect completion
        Ok(())
    }

    fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
        self.event_tx.subscribe()
    }

    async fn shutdown(&mut self) {
        if let Some(tx) = self.cmd_tx.take() {
            let _ = tx.send(AcpCmd::Shutdown).await;
        }
        if let Some(h) = self.thread_handle.take() {
            let _ = h.join();
        }
        eprintln!("[{}-acp] shutdown", self.agent_kind);
    }

    fn kind(&self) -> AgentKind {
        self.agent_kind
    }
}

/// Runs on a dedicated thread with a single-threaded tokio runtime + LocalSet.
fn run_acp_thread(
    agent_kind: AgentKind,
    cwd: PathBuf,
    event_tx: broadcast::Sender<AgentEvent>,
    cmd_rx: mpsc::Receiver<AcpCmd>,
    ready_tx: oneshot::Sender<Result<(), String>>,
    system_prompt: Option<String>,
) {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            let _ = ready_tx.send(Err(format!("Failed to build runtime: {}", e)));
            return;
        }
    };

    rt.block_on(async move {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async move {
                match acp_session_loop(agent_kind, cwd, event_tx, cmd_rx, ready_tx, system_prompt).await {
                    Ok(()) => {}
                    Err(e) => eprintln!("[{}-acp] session loop error: {}", agent_kind, e),
                }
            })
            .await;
    });
}

/// The actual ACP session lifecycle, running inside LocalSet.
/// Handles both Claude (via in-process duplex pipe) and Gemini (via subprocess stdio).
async fn acp_session_loop(
    agent_kind: AgentKind,
    cwd: PathBuf,
    event_tx: broadcast::Sender<AgentEvent>,
    mut cmd_rx: mpsc::Receiver<AcpCmd>,
    ready_tx: oneshot::Sender<Result<(), String>>,
    system_prompt: Option<String>,
) -> Result<(), String> {
    use agent_client_protocol as acp;
    use acp::Agent as _;
    use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

    // --- Write system prompt file for non-Claude agents (they read from files, not CLI flags) ---
    if let Some(ref prompt) = system_prompt {
        match agent_kind {
            AgentKind::Gemini => {
                // Gemini reads system prompt from GEMINI_SYSTEM_MD env var pointing to a file
                let prompt_path = cwd.join(".gemini").join("system.md");
                if let Some(parent) = prompt_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(&prompt_path, prompt);
            }
            AgentKind::OpenCode => {
                // OpenCode reads AGENTS.md from workspace root
                let _ = std::fs::write(cwd.join("AGENTS.md"), prompt);
            }
            AgentKind::Codex => {
                // Codex reads .codex/instructions.md from workspace
                let dir = cwd.join(".codex");
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::fs::write(dir.join("instructions.md"), prompt);
            }
            AgentKind::Claude => {} // handled via --system-prompt flag
        }
    }

    // --- Obtain the read/write streams depending on agent kind ---
    let (read_stream, write_stream, _claude_thread): (
        tokio::io::DuplexStream,
        tokio::io::DuplexStream,
        Option<std::thread::JoinHandle<()>>,
    ) = match agent_kind {
        AgentKind::Claude => {
            let (r, w, h) = claude_acp::spawn_claude_acp(cwd.clone(), system_prompt);
            (r, w, Some(h))
        }
        AgentKind::Gemini => {
            let system_md = system_prompt.as_ref().map(|_| cwd.join(".gemini").join("system.md"));
            let (r, w) = gemini_acp::spawn_gemini_process(&cwd, system_md.as_deref())?;
            (r, w, None)
        }
        AgentKind::OpenCode => {
            let (r, w) = opencode_acp::spawn_opencode_process(&cwd)?;
            (r, w, None)
        }
        AgentKind::Codex => {
            let (r, w) = codex_acp::spawn_codex_process(&cwd)?;
            (r, w, None)
        }
    };

    // --- Create ACP ClientSideConnection ---
    let client_handler = SharedAcpClientHandler {
        event_tx: event_tx.clone(),
    };
    let (conn, handle_io) = acp::ClientSideConnection::new(
        client_handler,
        write_stream.compat_write(),
        read_stream.compat(),
        |fut| {
            tokio::task::spawn_local(fut);
        },
    );
    tokio::task::spawn_local(handle_io);

    // --- Initialize ---
    eprintln!("[{}-acp] sending initialize...", agent_kind);
    let _init_resp = conn
        .initialize(
            acp::InitializeRequest::new(acp::ProtocolVersion::V1)
                .client_info(acp::Implementation::new("vibearound", "0.1.0").title("VibeAround")),
        )
        .await
        .map_err(|e| format!("ACP initialize failed: {}", e))?;
    eprintln!("[{}-acp] initialize ok", agent_kind);

    // --- Create session ---
    eprintln!("[{}-acp] creating session in {:?}...", agent_kind, &cwd);
    let session_resp = conn
        .new_session(acp::NewSessionRequest::new(cwd))
        .await
        .map_err(|e| format!("ACP new_session failed: {}", e))?;

    let session_id = session_resp.session_id;
    eprintln!("[{}-acp] session created: {:?}", agent_kind, session_id);

    let _ = ready_tx.send(Ok(()));

    // --- Command loop ---
    loop {
        let cmd = match cmd_rx.recv().await {
            Some(c) => c,
            None => break,
        };
        match cmd {
            AcpCmd::Prompt { text, done_tx } => {
                eprintln!("[{}-acp] sending prompt: {}", agent_kind, &text);
                let text_content = acp::ContentBlock::Text(acp::TextContent::new(text));
                let result = conn
                    .prompt(acp::PromptRequest::new(
                        session_id.clone(),
                        vec![text_content],
                    ))
                    .await;
                eprintln!("[{}-acp] prompt returned: {:?}", agent_kind, result.is_ok());
                match result {
                    Ok(_) => {
                        let _ = event_tx.send(AgentEvent::TurnComplete {
                            session_id: Some(session_id.to_string()),
                            cost_usd: None,
                        });
                        let _ = done_tx.send(Ok(()));
                    }
                    Err(e) => {
                        let err = format!("ACP prompt error: {}", e);
                        let _ = event_tx.send(AgentEvent::Error(err.clone()));
                        let _ = done_tx.send(Err(err));
                    }
                }
            }
            AcpCmd::Shutdown => break,
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Shared ACP Client handler — receives notifications from any ACP agent
// ---------------------------------------------------------------------------

/// Implements the ACP `Client` trait — translates ACP notifications into `AgentEvent`s.
/// Used by both Claude (via adapter) and Gemini (native ACP).
struct SharedAcpClientHandler {
    event_tx: broadcast::Sender<AgentEvent>,
}

#[async_trait::async_trait(?Send)]
impl agent_client_protocol::Client for SharedAcpClientHandler {
    async fn request_permission(
        &self,
        args: agent_client_protocol::RequestPermissionRequest,
    ) -> agent_client_protocol::Result<agent_client_protocol::RequestPermissionResponse> {
        // Auto-allow: pick the first option
        let option_id = args
            .options
            .first()
            .map(|o| o.option_id.clone())
            .unwrap_or_else(|| "allow".into());
        Ok(agent_client_protocol::RequestPermissionResponse::new(
            agent_client_protocol::RequestPermissionOutcome::Selected(
                agent_client_protocol::SelectedPermissionOutcome::new(option_id),
            ),
        ))
    }

    async fn session_notification(
        &self,
        args: agent_client_protocol::SessionNotification,
    ) -> agent_client_protocol::Result<()> {
        use agent_client_protocol::{ContentBlock, SessionUpdate};

        match args.update {
            SessionUpdate::AgentMessageChunk(chunk) => {
                if let ContentBlock::Text(t) = chunk.content {
                    let _ = self.event_tx.send(AgentEvent::Text(t.text));
                }
            }
            SessionUpdate::AgentThoughtChunk(chunk) => {
                if let ContentBlock::Text(t) = chunk.content {
                    let _ = self.event_tx.send(AgentEvent::Thinking(t.text));
                }
            }
            SessionUpdate::ToolCallUpdate(update) => {
                let name = update.fields.title.clone().unwrap_or_else(|| "unknown".into());
                let id = update.tool_call_id.to_string();
                // Check if this is a completed tool call (has status or raw_output)
                let has_output = update.fields.raw_output.is_some();
                let status_completed = update.fields.status.as_ref().map(|s| {
                    matches!(s, agent_client_protocol::ToolCallStatus::Completed | agent_client_protocol::ToolCallStatus::Failed)
                }).unwrap_or(false);

                if has_output || status_completed {
                    // This is a tool result update
                    let output = update.fields.raw_output.as_ref().map(|v| {
                        if let Some(s) = v.as_str() { s.to_string() } else { v.to_string() }
                    }).or_else(|| {
                        update.fields.content.as_ref().map(|blocks| {
                            blocks.iter().filter_map(|block| {
                                match block {
                                    agent_client_protocol::ToolCallContent::Content(c) => {
                                        if let ContentBlock::Text(t) = &c.content { Some(t.text.clone()) } else { None }
                                    }
                                    _ => None,
                                }
                            }).collect::<Vec<_>>().join("")
                        })
                    });
                    let is_error = matches!(update.fields.status.as_ref(), Some(agent_client_protocol::ToolCallStatus::Failed));
                    let _ = self.event_tx.send(AgentEvent::ToolResult { id, output, is_error });
                } else {
                    // This is a tool use start/progress update
                    let input = update.fields.raw_input.as_ref().map(|v| {
                        if let Some(s) = v.as_str() { s.to_string() } else { v.to_string() }
                    });
                    let _ = self.event_tx.send(AgentEvent::ToolUse { name, id, input });
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn write_text_file(
        &self,
        _: agent_client_protocol::WriteTextFileRequest,
    ) -> agent_client_protocol::Result<agent_client_protocol::WriteTextFileResponse> {
        Err(agent_client_protocol::Error::method_not_found())
    }

    async fn read_text_file(
        &self,
        _: agent_client_protocol::ReadTextFileRequest,
    ) -> agent_client_protocol::Result<agent_client_protocol::ReadTextFileResponse> {
        Err(agent_client_protocol::Error::method_not_found())
    }

    async fn create_terminal(
        &self,
        _: agent_client_protocol::CreateTerminalRequest,
    ) -> agent_client_protocol::Result<agent_client_protocol::CreateTerminalResponse> {
        Err(agent_client_protocol::Error::method_not_found())
    }

    async fn terminal_output(
        &self,
        _: agent_client_protocol::TerminalOutputRequest,
    ) -> agent_client_protocol::Result<agent_client_protocol::TerminalOutputResponse> {
        Err(agent_client_protocol::Error::method_not_found())
    }

    async fn release_terminal(
        &self,
        _: agent_client_protocol::ReleaseTerminalRequest,
    ) -> agent_client_protocol::Result<agent_client_protocol::ReleaseTerminalResponse> {
        Err(agent_client_protocol::Error::method_not_found())
    }

    async fn wait_for_terminal_exit(
        &self,
        _: agent_client_protocol::WaitForTerminalExitRequest,
    ) -> agent_client_protocol::Result<agent_client_protocol::WaitForTerminalExitResponse> {
        Err(agent_client_protocol::Error::method_not_found())
    }

    async fn kill_terminal_command(
        &self,
        _: agent_client_protocol::KillTerminalCommandRequest,
    ) -> agent_client_protocol::Result<agent_client_protocol::KillTerminalCommandResponse> {
        Err(agent_client_protocol::Error::method_not_found())
    }

    async fn ext_method(
        &self,
        _: agent_client_protocol::ExtRequest,
    ) -> agent_client_protocol::Result<agent_client_protocol::ExtResponse> {
        Err(agent_client_protocol::Error::method_not_found())
    }

    async fn ext_notification(
        &self,
        _: agent_client_protocol::ExtNotification,
    ) -> agent_client_protocol::Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// JSONL-based backend — for Codex (per-prompt subprocess with --json output)
// and OpenCode (opencode run --format json, per-prompt subprocess)
// ---------------------------------------------------------------------------

use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};

/// Subprocess-per-prompt agent backend for OpenCode and Codex.
/// Each `send_message` spawns a new subprocess, reads JSONL/text from stdout,
/// and waits for it to exit before returning.
pub struct JsonlBackend {
    agent_kind: AgentKind,
    event_tx: broadcast::Sender<AgentEvent>,
    cwd: Option<PathBuf>,
}

impl JsonlBackend {
    pub fn new(agent_kind: AgentKind) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        Self { agent_kind, event_tx, cwd: None }
    }
}

#[async_trait::async_trait]
impl AgentBackend for JsonlBackend {
    async fn start(&mut self, cwd: &Path, _system_prompt: Option<&str>) -> Result<(), String> {
        // Just verify the CLI exists
        let cmd = match self.agent_kind {
            AgentKind::OpenCode => "opencode",
            AgentKind::Codex => "codex",
            _ => unreachable!(),
        };
        let check = tokio::process::Command::new("which")
            .arg(cmd)
            .output()
            .await
            .map_err(|e| format!("Failed to check {}: {}", cmd, e))?;
        if !check.status.success() {
            return Err(format!("{} not found in PATH", cmd));
        }
        self.cwd = Some(cwd.to_path_buf());
        eprintln!("[{}-jsonl] ready (per-prompt mode)", self.agent_kind);
        Ok(())
    }

    async fn send_message(&self, text: &str) -> Result<(), String> {
        let cwd = self.cwd.as_ref().ok_or("Agent not started")?;
        let event_tx = self.event_tx.clone();
        let agent_kind = self.agent_kind;

        let (cmd, args): (&str, Vec<String>) = match agent_kind {
            AgentKind::OpenCode => ("opencode", vec![
                "run".into(), "--format".into(), "json".into(), "--".into(), text.to_string(),
            ]),
            AgentKind::Codex => ("codex", vec![
                "exec".into(), "--json".into(), "--full-auto".into(), text.to_string(),
            ]),
            _ => unreachable!(),
        };

        eprintln!("[{}-jsonl] spawning: {} {:?}", agent_kind, cmd, &args[..args.len().min(3)]);

        let mut child = tokio::process::Command::new(cmd)
            .args(&args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("Failed to spawn {}: {}", cmd, e))?;

        let stdout = child.stdout.take().ok_or("No stdout")?;

        // Read JSONL lines from stdout, parse into AgentEvents
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() { continue; }
            let msg: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => {
                    // Not JSON — treat as plain text output
                    if !line.trim().is_empty() {
                        let _ = event_tx.send(AgentEvent::Text(line));
                    }
                    continue;
                }
            };
            match agent_kind {
                AgentKind::OpenCode => opencode_jsonl::parse_event(&msg, &event_tx),
                AgentKind::Codex => codex_jsonl::parse_event(&msg, &event_tx),
                _ => {}
            }
        }

        // Wait for process to exit
        let status = child.wait().await.map_err(|e| format!("{} wait: {}", cmd, e))?;
        eprintln!("[{}-jsonl] process exited: {}", agent_kind, status);

        // Emit TurnComplete so the worker's event loop knows we're done
        let _ = event_tx.send(AgentEvent::TurnComplete { session_id: None, cost_usd: None });

        Ok(())
    }

    async fn send_message_fire(&self, text: &str) -> Result<(), String> {
        let cwd = self.cwd.as_ref().ok_or("Agent not started")?.clone();
        let event_tx = self.event_tx.clone();
        let agent_kind = self.agent_kind;
        let text = text.to_string();

        tokio::spawn(async move {
            let (cmd, args): (&str, Vec<String>) = match agent_kind {
                AgentKind::OpenCode => ("opencode", vec![
                    "run".into(), "--format".into(), "json".into(), "--".into(), text,
                ]),
                AgentKind::Codex => ("codex", vec![
                    "exec".into(), "--json".into(), "--full-auto".into(), text,
                ]),
                _ => unreachable!(),
            };

            eprintln!("[{}-jsonl] spawning (fire): {} {:?}", agent_kind, cmd, &args[..args.len().min(3)]);

            let mut child = match tokio::process::Command::new(cmd)
                .args(&args)
                .current_dir(&cwd)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .kill_on_drop(true)
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    let _ = event_tx.send(AgentEvent::Error(format!("Failed to spawn {}: {}", cmd, e)));
                    let _ = event_tx.send(AgentEvent::TurnComplete { session_id: None, cost_usd: None });
                    return;
                }
            };

            if let Some(stdout) = child.stdout.take() {
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if line.trim().is_empty() { continue; }
                    match serde_json::from_str::<serde_json::Value>(&line) {
                        Ok(msg) => match agent_kind {
                            AgentKind::OpenCode => opencode_jsonl::parse_event(&msg, &event_tx),
                            AgentKind::Codex => codex_jsonl::parse_event(&msg, &event_tx),
                            _ => {}
                        },
                        Err(_) => {
                            if !line.trim().is_empty() {
                                let _ = event_tx.send(AgentEvent::Text(line));
                            }
                        }
                    }
                }
            }

            let _ = child.wait().await;
            let _ = event_tx.send(AgentEvent::TurnComplete { session_id: None, cost_usd: None });
        });

        Ok(())
    }

    fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
        self.event_tx.subscribe()
    }

    async fn shutdown(&mut self) {
        self.cwd = None;
        eprintln!("[{}-jsonl] shutdown", self.agent_kind);
    }

    fn kind(&self) -> AgentKind {
        self.agent_kind
    }
}
