//! PTY session manager: create, track, query, attach, and delete terminal sessions.

use std::io::Write;
use std::sync::Arc;

use bytes::Bytes;
use serde::Serialize;
use tokio::sync::broadcast;

use super::runtime::{spawn_pty, PtyRunState, PtyTool, ResizeSender};
use super::session::{
    unix_now_secs, CircularBuffer, Registry, SessionContext, SessionId, SessionMetadata,
    LIVE_BROADCAST_CAP,
};

pub struct PtySessionManager {
    registry: Registry,
}

#[derive(Debug, Clone, Serialize)]
pub struct PtySessionSummary {
    pub session_id: String,
    pub tool: PtyTool,
    pub status: PtyRunState,
    pub created_at: u64,
    pub project_path: Option<String>,
    pub tmux_session: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PtySessionCreated {
    pub session_id: String,
    pub tool: PtyTool,
    pub created_at: u64,
    pub project_path: Option<String>,
}

pub struct PtyAttachHandles {
    pub buffer: Arc<CircularBuffer>,
    pub state: Arc<std::sync::RwLock<PtyRunState>>,
    pub live_tx: broadcast::Sender<Bytes>,
    pub writer: Arc<std::sync::Mutex<Box<dyn Write + Send>>>,
    pub resize_tx: ResizeSender,
}

impl PtySessionManager {
    pub fn new() -> Self {
        Self {
            registry: Arc::new(dashmap::DashMap::new()),
        }
    }

    pub fn from_registry(registry: Registry) -> Self {
        Self { registry }
    }

    pub fn registry(&self) -> Registry {
        Arc::clone(&self.registry)
    }

    pub fn list_sessions(&self) -> Vec<PtySessionSummary> {
        let mut items = Vec::new();
        for entry in self.registry.iter() {
            let sid = entry.key();
            let ctx = entry.value();
            let status = ctx
                .state
                .read()
                .map(|g| g.clone())
                .unwrap_or(PtyRunState::Running {
                    tool: ctx.metadata.tool,
                });
            items.push(PtySessionSummary {
                session_id: sid.0.to_string(),
                tool: ctx.metadata.tool,
                status,
                created_at: ctx.metadata.created_at,
                project_path: ctx.metadata.project_path.clone(),
                tmux_session: ctx.metadata.tmux_session.clone(),
            });
        }
        items
    }

    pub fn create_session(
        &self,
        tool: PtyTool,
        project_path: Option<String>,
        tmux_session: Option<String>,
        theme: Option<String>,
        initial_size: Option<(u16, u16)>,
    ) -> Result<PtySessionCreated, String> {
        let cwd = project_path.as_ref().map(std::path::PathBuf::from);
        let (bridge, mut pty_rx, resize_tx, mut state_rx) = spawn_pty(
            tool,
            cwd,
            tmux_session.clone(),
            theme,
            initial_size,
        )
        .map_err(|e| format!("Failed to spawn PTY: {}", e))?;

        let session_id = SessionId::new();
        let metadata = SessionMetadata {
            created_at: unix_now_secs(),
            project_path: project_path.clone(),
            tool,
            tmux_session,
        };

        let buffer = Arc::new(CircularBuffer::new());
        let (live_tx, _) = broadcast::channel(LIVE_BROADCAST_CAP);
        let run_state: Arc<std::sync::RwLock<PtyRunState>> =
            Arc::new(std::sync::RwLock::new(PtyRunState::Running { tool }));

        let ctx = SessionContext {
            bridge,
            resize_tx,
            state: Arc::clone(&run_state),
            metadata: metadata.clone(),
            buffer: Arc::clone(&buffer),
            live_tx: live_tx.clone(),
        };
        self.registry.insert(session_id, ctx);

        let buf_clone = Arc::clone(&buffer);
        let tx_clone = live_tx.clone();
        tokio::spawn(async move {
            while let Some(data) = pty_rx.recv().await {
                buf_clone.push(&data);
                let _ = tx_clone.send(Bytes::from(data));
            }
        });

        let rs = Arc::clone(&run_state);
        tokio::spawn(async move {
            while let Some(new_state) = state_rx.recv().await {
                if let Ok(mut g) = rs.write() {
                    *g = new_state;
                }
            }
        });

        Ok(PtySessionCreated {
            session_id: session_id.0.to_string(),
            tool: metadata.tool,
            created_at: metadata.created_at,
            project_path: metadata.project_path,
        })
    }

    pub fn delete_session(&self, session_id: SessionId) -> bool {
        if let Some((_, ctx)) = self.registry.remove(&session_id) {
            let _ = ctx.bridge.kill();
            true
        } else {
            false
        }
    }

    pub fn attach_handles(&self, session_id: SessionId) -> Option<PtyAttachHandles> {
        let ctx = self.registry.get(&session_id)?;
        Some(PtyAttachHandles {
            buffer: Arc::clone(&ctx.buffer),
            state: Arc::clone(&ctx.state),
            live_tx: ctx.live_tx.clone(),
            writer: Arc::clone(&ctx.bridge.writer),
            resize_tx: ctx.resize_tx.clone(),
        })
    }
}
