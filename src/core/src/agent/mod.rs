//! Unified agent backend: all agents communicate via ACP (Agent Client Protocol).
//!
//! - Gemini: speaks ACP natively (`gemini --experimental-acp`)
//! - Claude: wrapped via `claude_acp` adapter (Claude SDK protocol → ACP translation)
//!
//! The worker only sees `AgentBackend` + `AgentEvent`, backed by ACP `ClientSideConnection`.

pub mod claude_acp;
pub mod claude_sdk;
pub mod gemini_acp;

use std::fmt;
use std::path::Path;

/// Which agent CLI to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Claude,
    Gemini,
}

impl fmt::Display for AgentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentKind::Claude => write!(f, "claude"),
            AgentKind::Gemini => write!(f, "gemini"),
        }
    }
}

impl AgentKind {
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "claude" | "claude-code" => Some(Self::Claude),
            "gemini" | "gemini-cli" => Some(Self::Gemini),
            _ => None,
        }
    }
}

/// Events emitted by an agent backend, consumed by the IM worker.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// Assistant text chunk (streaming).
    Text(String),
    /// Progress indicator: "Thinking...", "Using tool: Bash", etc.
    Progress(String),
    /// A tool call started.
    ToolUse { name: String },
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
    async fn start(&mut self, cwd: &Path) -> Result<(), String>;

    /// Send a user message via ACP `prompt()`. Events are delivered via `subscribe()`.
    async fn send_message(&self, text: &str) -> Result<(), String>;

    /// Subscribe to the agent's event stream.
    fn subscribe(&self) -> tokio::sync::broadcast::Receiver<AgentEvent>;

    /// Gracefully shut down the agent subprocess.
    async fn shutdown(&mut self);

    /// Which agent kind this backend represents.
    fn kind(&self) -> AgentKind;
}

/// Create a new (unstarted) agent backend for the given kind.
/// Both return an ACP-backed implementation.
pub fn create_backend(kind: AgentKind) -> Box<dyn AgentBackend> {
    match kind {
        AgentKind::Claude => Box::new(AcpBackend::new(AgentKind::Claude)),
        AgentKind::Gemini => Box::new(AcpBackend::new(AgentKind::Gemini)),
    }
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
    async fn start(&mut self, cwd: &Path) -> Result<(), String> {
        let cwd = cwd.to_path_buf();
        let event_tx = self.event_tx.clone();
        let agent_kind = self.agent_kind;
        let (cmd_tx, cmd_rx) = mpsc::channel::<AcpCmd>(32);
        let (ready_tx, ready_rx) = oneshot::channel::<Result<(), String>>();

        let handle = std::thread::Builder::new()
            .name(format!("{}-acp", agent_kind))
            .spawn(move || {
                run_acp_thread(agent_kind, cwd, event_tx, cmd_rx, ready_tx);
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
                match acp_session_loop(agent_kind, cwd, event_tx, cmd_rx, ready_tx).await {
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
) -> Result<(), String> {
    use agent_client_protocol as acp;
    use acp::Agent as _;
    use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

    // --- Obtain the read/write streams depending on agent kind ---
    let (read_stream, write_stream, _claude_thread): (
        tokio::io::DuplexStream,
        tokio::io::DuplexStream,
        Option<std::thread::JoinHandle<()>>,
    ) = match agent_kind {
        AgentKind::Claude => {
            let (r, w, h) = claude_acp::spawn_claude_acp(cwd.clone());
            (r, w, Some(h))
        }
        AgentKind::Gemini => {
            let (r, w) = gemini_acp::spawn_gemini_process(&cwd)?;
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
    let _init_resp = conn
        .initialize(
            acp::InitializeRequest::new(acp::ProtocolVersion::V1)
                .client_info(acp::Implementation::new("vibearound", "0.1.0").title("VibeAround")),
        )
        .await
        .map_err(|e| format!("ACP initialize failed: {}", e))?;

    // --- Create session ---
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
                let text_content = acp::ContentBlock::Text(acp::TextContent::new(text));
                let result = conn
                    .prompt(acp::PromptRequest::new(
                        session_id.clone(),
                        vec![text_content],
                    ))
                    .await;
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
            SessionUpdate::AgentThoughtChunk(_) => {
                let _ = self.event_tx.send(AgentEvent::Progress("Thinking...".into()));
            }
            SessionUpdate::ToolCallUpdate(update) => {
                if let Some(ref title) = update.fields.title {
                    let _ = self.event_tx.send(AgentEvent::ToolUse {
                        name: title.clone(),
                    });
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
