//! PTY session manager: create, track, query, attach, and delete terminal sessions.

use std::io::Write;
use std::sync::Arc;

use anyhow::Context;

use bytes::Bytes;
use serde::Serialize;
use tokio::sync::{broadcast, mpsc};

use super::runtime::{
    spawn_pty, spawn_pty_with_command, PtyBridge, PtyRunState, PtyTool, ResizeSender,
};
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
    pub profile_id: Option<String>,
    pub profile_label: Option<String>,
    pub launch_target: Option<String>,
    pub tmux_session: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PtySessionCreated {
    pub session_id: String,
    pub tool: PtyTool,
    pub created_at: u64,
    pub project_path: Option<String>,
    pub profile_id: Option<String>,
    pub profile_label: Option<String>,
    pub launch_target: Option<String>,
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
                profile_id: ctx.metadata.profile_id.clone(),
                profile_label: ctx.metadata.profile_label.clone(),
                launch_target: ctx.metadata.launch_target.clone(),
                tmux_session: ctx.metadata.tmux_session.clone(),
            });
        }
        items
    }

    fn register_session(
        &self,
        metadata: SessionMetadata,
        bridge: PtyBridge,
        mut pty_rx: mpsc::Receiver<Vec<u8>>,
        resize_tx: ResizeSender,
        mut state_rx: mpsc::Receiver<PtyRunState>,
    ) -> PtySessionCreated {
        let session_id = SessionId::new();
        let buffer = Arc::new(CircularBuffer::new());
        let (live_tx, _) = broadcast::channel(LIVE_BROADCAST_CAP);
        let run_state: Arc<std::sync::RwLock<PtyRunState>> =
            Arc::new(std::sync::RwLock::new(PtyRunState::Running {
                tool: metadata.tool,
            }));

        let created = PtySessionCreated {
            session_id: session_id.0.to_string(),
            tool: metadata.tool,
            created_at: metadata.created_at,
            project_path: metadata.project_path.clone(),
            profile_id: metadata.profile_id.clone(),
            profile_label: metadata.profile_label.clone(),
            launch_target: metadata.launch_target.clone(),
        };

        let ctx = SessionContext {
            bridge,
            resize_tx,
            state: Arc::clone(&run_state),
            metadata,
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

        created
    }

    pub fn create_session(
        &self,
        tool: PtyTool,
        project_path: Option<String>,
        tmux_session: Option<String>,
        theme: Option<String>,
        initial_size: Option<(u16, u16)>,
    ) -> anyhow::Result<PtySessionCreated> {
        let cwd = project_path.as_ref().map(std::path::PathBuf::from);
        let (bridge, pty_rx, resize_tx, state_rx) =
            spawn_pty(tool, cwd, tmux_session.clone(), theme, initial_size)
                .context("Failed to spawn PTY")?;

        let metadata = SessionMetadata {
            created_at: unix_now_secs(),
            project_path: project_path.clone(),
            tool,
            profile_id: None,
            profile_label: None,
            launch_target: None,
            tmux_session,
        };

        Ok(self.register_session(metadata, bridge, pty_rx, resize_tx, state_rx))
    }

    pub fn create_profile_session(
        &self,
        tool: PtyTool,
        command: String,
        env: Vec<(String, String)>,
        profile_id: String,
        profile_label: String,
        launch_target: String,
        project_path: Option<String>,
        theme: Option<String>,
        initial_size: Option<(u16, u16)>,
    ) -> anyhow::Result<PtySessionCreated> {
        self.create_command_session(
            tool,
            command,
            env,
            Some(profile_id),
            Some(profile_label),
            Some(launch_target),
            project_path,
            theme,
            initial_size,
        )
    }

    pub fn create_command_session(
        &self,
        tool: PtyTool,
        command: String,
        env: Vec<(String, String)>,
        profile_id: Option<String>,
        profile_label: Option<String>,
        launch_target: Option<String>,
        project_path: Option<String>,
        theme: Option<String>,
        initial_size: Option<(u16, u16)>,
    ) -> anyhow::Result<PtySessionCreated> {
        let cwd = project_path.as_ref().map(std::path::PathBuf::from);
        let (bridge, pty_rx, resize_tx, state_rx) =
            spawn_pty_with_command(tool, cwd, None, theme, initial_size, Some(command), env)
                .context("Failed to spawn command PTY")?;

        let metadata = SessionMetadata {
            created_at: unix_now_secs(),
            project_path: project_path.clone(),
            tool,
            profile_id,
            profile_label,
            launch_target,
            tmux_session: None,
        };

        Ok(self.register_session(metadata, bridge, pty_rx, resize_tx, state_rx))
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

impl Default for PtySessionManager {
    fn default() -> Self {
        Self::new()
    }
}
