//! Claude ACP Adapter — pure translation layer between ACP and `ClaudeSdk`.
//!
//! Architecture:
//!   Worker ←→ ClientSideConnection ←→ [in-process duplex] ←→ AgentSideConnection ←→ this adapter ←→ ClaudeSdk ←→ claude CLI
//!
//! This module does NOT know how to talk to the Claude CLI directly.
//! It delegates all CLI communication to `claude_sdk::ClaudeSdk` and translates
//! `SdkEvent`s into ACP `SessionNotification`s.

use std::path::PathBuf;
use std::rc::Rc;

use agent_client_protocol as acp;
use tokio::sync::mpsc;

use super::claude_sdk::{ClaudeSdk, ContentBlock, SdkEvent};

/// Spawn a Claude ACP agent on a dedicated thread (required because `ClaudeSdk` uses `spawn_local`).
/// Returns the client-side halves of a duplex pipe for `ClientSideConnection`.
pub fn spawn_claude_acp(
    cwd: PathBuf,
) -> (
    tokio::io::DuplexStream, // client reads from this
    tokio::io::DuplexStream, // client writes to this
    std::thread::JoinHandle<()>,
) {
    let (client_read, agent_write) = tokio::io::duplex(64 * 1024);
    let (agent_read, client_write) = tokio::io::duplex(64 * 1024);

    let handle = std::thread::Builder::new()
        .name("claude-acp".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to build claude-acp runtime");

            rt.block_on(async move {
                let local = tokio::task::LocalSet::new();
                local
                    .run_until(async move {
                        if let Err(e) = run_acp_bridge(cwd, agent_read, agent_write).await {
                            eprintln!("[claude-acp] bridge error: {}", e);
                        }
                    })
                    .await;
            });
        })
        .expect("Failed to spawn claude-acp thread");

    (client_read, client_write, handle)
}

// ---------------------------------------------------------------------------
// ACP bridge — connects AgentSideConnection to ClaudeSdk
// ---------------------------------------------------------------------------

async fn run_acp_bridge(
    cwd: PathBuf,
    agent_read: tokio::io::DuplexStream,
    agent_write: tokio::io::DuplexStream,
) -> Result<(), String> {
    use acp::Client as _;
    use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

    // Notification channel: event translator → ACP connection
    let (notif_tx, mut notif_rx) = mpsc::channel::<acp::SessionNotification>(256);

    let agent_impl = ClaudeAcpBridge::new(cwd.clone(), notif_tx);

    let (conn, handle_io) = acp::AgentSideConnection::new(
        agent_impl,
        agent_write.compat_write(),
        agent_read.compat(),
        |fut| {
            tokio::task::spawn_local(fut);
        },
    );

    // Drain notification channel → ACP connection
    let conn = Rc::new(conn);
    let conn_for_notif = conn.clone();
    tokio::task::spawn_local(async move {
        while let Some(notif) = notif_rx.recv().await {
            if conn_for_notif.session_notification(notif).await.is_err() {
                break;
            }
        }
    });

    handle_io.await.map_err(|e| format!("ACP IO error: {}", e))
}

// ---------------------------------------------------------------------------
// ClaudeAcpBridge — ACP Agent that delegates to ClaudeSdk
// ---------------------------------------------------------------------------

struct ClaudeAcpBridge {
    cwd: PathBuf,
    notif_tx: mpsc::Sender<acp::SessionNotification>,
    /// The underlying SDK handle, created on first `initialize`.
    sdk: tokio::sync::Mutex<Option<ClaudeSdk>>,
}

impl ClaudeAcpBridge {
    fn new(cwd: PathBuf, notif_tx: mpsc::Sender<acp::SessionNotification>) -> Self {
        Self {
            cwd,
            notif_tx,
            sdk: tokio::sync::Mutex::new(None),
        }
    }

    async fn ensure_sdk(&self) -> Result<(), acp::Error> {
        let mut lock = self.sdk.lock().await;
        if lock.is_some() {
            return Ok(());
        }
        let sdk = ClaudeSdk::spawn(&self.cwd).await
            .map_err(|e| acp::Error::new(-32603, e))?;
        *lock = Some(sdk);
        Ok(())
    }

    /// Translate SDK events into ACP notifications until a TurnResult arrives.
    async fn drain_until_turn_result(&self, session_id: &str) -> Result<(bool, Option<String>), acp::Error> {
        let lock = self.sdk.lock().await;
        let sdk = lock.as_ref().ok_or_else(|| acp::Error::new(-32603, "SDK not running"))?;

        loop {
            let event = sdk.recv_event().await
                .ok_or_else(|| acp::Error::new(-32603, "SDK event stream ended"))?;

            match event {
                SdkEvent::AssistantMessage { content } => {
                    for block in content {
                        let notif = translate_content_block(session_id, &block);
                        let _ = self.notif_tx.send(notif).await;
                    }
                }
                SdkEvent::TurnResult { is_error, error_text, .. } => {
                    return Ok((is_error, error_text));
                }
                SdkEvent::SystemInit { .. } | SdkEvent::ControlHandled { .. } => {
                    // informational, no ACP notification needed
                }
            }
        }
    }
}

#[async_trait::async_trait(?Send)]
impl acp::Agent for ClaudeAcpBridge {
    async fn initialize(&self, _args: acp::InitializeRequest) -> acp::Result<acp::InitializeResponse> {
        self.ensure_sdk().await?;
        Ok(acp::InitializeResponse::new(acp::ProtocolVersion::V1))
    }

    async fn authenticate(&self, _args: acp::AuthenticateRequest) -> acp::Result<acp::AuthenticateResponse> {
        Ok(acp::AuthenticateResponse::default())
    }

    async fn new_session(&self, _args: acp::NewSessionRequest) -> acp::Result<acp::NewSessionResponse> {
        let lock = self.sdk.lock().await;
        let sid = match lock.as_ref() {
            Some(sdk) => sdk.session_id().await,
            None => "claude-0".to_string(),
        };
        Ok(acp::NewSessionResponse::new(sid))
    }

    async fn load_session(&self, _args: acp::LoadSessionRequest) -> acp::Result<acp::LoadSessionResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn set_session_mode(&self, _args: acp::SetSessionModeRequest) -> acp::Result<acp::SetSessionModeResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn prompt(&self, args: acp::PromptRequest) -> acp::Result<acp::PromptResponse> {
        self.ensure_sdk().await?;

        // Extract text from ACP content blocks
        let text = args.prompt.iter().filter_map(|block| match block {
            acp::ContentBlock::Text(t) => Some(t.text.as_str()),
            _ => None,
        }).collect::<Vec<_>>().join("\n");

        // Send to Claude SDK
        {
            let lock = self.sdk.lock().await;
            let sdk = lock.as_ref().ok_or_else(|| acp::Error::new(-32603, "SDK not running"))?;
            sdk.send_user_message(&text).await
                .map_err(|e| acp::Error::new(-32603, e))?;
        }

        // Get session ID for notifications
        let sid = {
            let lock = self.sdk.lock().await;
            match lock.as_ref() {
                Some(sdk) => sdk.session_id().await,
                None => "claude-0".to_string(),
            }
        };

        // Drain events until turn completes
        let (is_error, error_text) = self.drain_until_turn_result(&sid).await?;

        if is_error {
            return Err(acp::Error::new(-32603, error_text.unwrap_or_else(|| "Unknown error".into())));
        }

        Ok(acp::PromptResponse::new(acp::StopReason::EndTurn))
    }

    async fn cancel(&self, _args: acp::CancelNotification) -> acp::Result<()> {
        Ok(())
    }

    async fn set_session_config_option(&self, _args: acp::SetSessionConfigOptionRequest) -> acp::Result<acp::SetSessionConfigOptionResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn ext_method(&self, _args: acp::ExtRequest) -> acp::Result<acp::ExtResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn ext_notification(&self, _args: acp::ExtNotification) -> acp::Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Translation helpers — SdkEvent → ACP SessionNotification
// ---------------------------------------------------------------------------

fn translate_content_block(session_id: &str, block: &ContentBlock) -> acp::SessionNotification {
    match block {
        ContentBlock::Text { text } => acp::SessionNotification::new(
            session_id.to_string(),
            acp::SessionUpdate::AgentMessageChunk(
                acp::ContentChunk::new(acp::ContentBlock::Text(
                    acp::TextContent::new(text),
                )),
            ),
        ),
        ContentBlock::Thinking { text } => acp::SessionNotification::new(
            session_id.to_string(),
            acp::SessionUpdate::AgentThoughtChunk(
                acp::ContentChunk::new(acp::ContentBlock::Text(
                    acp::TextContent::new(text),
                )),
            ),
        ),
        ContentBlock::ToolUse { id, name } => {
            let fields = acp::ToolCallUpdateFields::new().title(name.clone());
            acp::SessionNotification::new(
                session_id.to_string(),
                acp::SessionUpdate::ToolCallUpdate(
                    acp::ToolCallUpdate::new(id.clone(), fields),
                ),
            )
        }
    }
}
