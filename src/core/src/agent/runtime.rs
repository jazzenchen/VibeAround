//! `Agent` — one live ACP connection to a coding CLI process.
//!
//! Each `Agent` wraps a single ACP `ConnectionTo<acp::Agent>` to a real
//! agent subprocess. Northbound callers ([`Conversation`]) use explicit
//! methods on this type; southbound client events (`session_notification`,
//! `request_permission`) are forwarded to a caller-supplied
//! [`AgentClientHandler`].
//!
//! Only stdio ACP is supported — no provider trait, no pluggable
//! transport. If another transport is ever needed, reintroduce a trait
//! at that time.
//!
//! ## Lifecycle
//!
//! Spawn and supervision are delegated to [`process::Supervisor`]. The
//! agent's [`RestartPolicy`] is `Never` — crashes surface via the normal
//! supervisor broadcast and it's the owning [`Conversation`]'s decision
//! whether to re-spawn. `Agent::shutdown` translates to
//! `supervisor.force_stop(process_id)`.
//!
//! [`ThreadRuntime`]: crate::workspace::threads::ThreadRuntime
//! [`process::Supervisor`]: crate::process::Supervisor
//! [`RestartPolicy`]: crate::process::RestartPolicy

use std::path::Path;
use std::sync::{Arc, OnceLock};

use anyhow::{anyhow, Context};
use tokio::sync::{oneshot, Mutex};

use acp::schema;
use agent_client_protocol as acp;

use crate::process::bridge::{BridgeFactory, ProcessBridge};
use crate::process::registry::ProcessKind;
use crate::process::supervisor::{ProcessId, RestartPolicy, SpawnSpec, Supervisor};
use crate::routing::RouteKey;

use super::bridge::AcpAgentBridge;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Callback trait for southbound ACP client events forwarded to the caller.
///
/// The agent calls these methods when the real CLI sends notifications or
/// requests back through the ACP client channel.
#[async_trait::async_trait]
pub trait AgentClientHandler: Send + Sync + 'static {
    async fn session_notification(&self, args: schema::SessionNotification) -> acp::Result<()>;

    async fn request_permission(
        &self,
        args: schema::RequestPermissionRequest,
    ) -> acp::Result<schema::RequestPermissionResponse>;

    async fn prompt_finished(&self, _success: bool) -> acp::Result<()> {
        Ok(())
    }
}

/// Handle returned from a successful [`Agent::spawn`].
pub struct AgentReady {
    pub agent: Arc<Agent>,
    pub startup_session_id: Option<String>,
    pub startup_modes: Option<schema::SessionModeState>,
    pub startup_config_options: Option<Vec<schema::SessionConfigOption>>,
    pub initialize: schema::InitializeResponse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupSession {
    Fresh,
    Load(String),
    Resume(String),
}

impl StartupSession {
    pub fn session_id(&self) -> Option<&str> {
        match self {
            Self::Fresh => None,
            Self::Load(session_id) | Self::Resume(session_id) => Some(session_id.as_str()),
        }
    }
}

/// One live ACP-speaking coding CLI.
pub struct Agent {
    /// The southbound ACP connection to the real agent process.
    conn: acp::ConnectionTo<acp::Agent>,
    agent_id: String,
    /// ACP initialize response from first startup.
    initialize: schema::InitializeResponse,
    /// ACP session ID obtained from new_session / load_session.
    session_id: Mutex<Option<String>>,
    /// Supervisor handle installed by [`Agent::spawn`] after registration.
    /// `None` until the registration returns — effectively
    /// a moment-of-initialization gap where `shutdown()` is a no-op.
    process_id: OnceLock<ProcessId>,
}

impl Agent {
    /// Spawn a new agent through the process supervisor.
    ///
    /// `agent_id` must match an entry in `resources/agents.json`. The
    /// binary is lazily installed on first miss (npm or `install_cmd`).
    ///
    /// `route` is used to build a supervisor label unique to the owning
    /// conversation (`<agent_id>:<channel_kind>:<chat_id>`) so that
    /// multiple concurrent agents of the same kind remain distinguishable
    /// in snapshots and logs.
    pub async fn spawn(
        agent_id: String,
        route: &RouteKey,
        workspace: &Path,
        startup_session: StartupSession,
        client_handler: Arc<dyn AgentClientHandler>,
        extra_args: Vec<String>,
        extra_env: Vec<(String, String)>,
    ) -> anyhow::Result<AgentReady> {
        let cwd = workspace.to_path_buf();
        let label = format!("{}:{}", agent_id, route);

        super::auto_install_project_integrations(&agent_id, workspace)
            .with_context(|| format!("install project integrations for {}", agent_id))?;

        // Resolve program + args + install if needed.
        let (program, mut resolved_args) = resolve_agent_program(&agent_id).await?;
        resolved_args.extend(extra_args);
        tracing::info!(
            "[{}] spawning {} {} in {:?}",
            label,
            program,
            resolved_args.join(" "),
            cwd
        );

        let mut spec = SpawnSpec::new(program).args(resolved_args).cwd(cwd.clone());
        for (k, v) in extra_env {
            spec = spec.env(k, v);
        }

        // The supervisor's factory is `Fn()`, so one-shot state (the
        // ready sender + client handler) has to go through a Mutex<Option>
        // that the first (and only) invocation drains. RestartPolicy::Never
        // guarantees there is no second invocation.
        let (ready_tx, ready_rx) = oneshot::channel::<anyhow::Result<AgentReady>>();
        let bridge = AcpAgentBridge {
            agent_id: agent_id.clone(),
            cwd,
            startup_session,
            client_handler,
            ready_tx,
        };
        let slot: Arc<parking_lot::Mutex<Option<AcpAgentBridge>>> =
            Arc::new(parking_lot::Mutex::new(Some(bridge)));
        let factory: BridgeFactory = Box::new(move || {
            let b = slot.lock().take().expect(
                "AcpAgentBridge factory called more than once — RestartPolicy::Never guarantees single-spawn",
            );
            Box::new(b) as Box<dyn ProcessBridge>
        });

        let id = Supervisor::global().register(
            ProcessKind::AcpAgent,
            label,
            spec,
            RestartPolicy::Never,
            factory,
        );

        let err_label = agent_id.clone();
        let ready = ready_rx
            .await
            .map_err(|_| anyhow!("Agent bridge for {} died during init", err_label))??;

        // Stash the id back onto the Agent so shutdown() can reach it.
        let _ = ready.agent.process_id.set(id);

        Ok(ready)
    }

    /// Constructor used by the bridge once the ACP handshake has succeeded.
    /// Not `pub` externally — only `agent::bridge` needs it.
    pub(crate) fn from_connection(
        conn: acp::ConnectionTo<acp::Agent>,
        agent_id: String,
        initialize: schema::InitializeResponse,
        startup_session_id: Option<String>,
    ) -> Arc<Self> {
        Arc::new(Self {
            conn,
            agent_id,
            initialize,
            session_id: Mutex::new(startup_session_id),
            process_id: OnceLock::new(),
        })
    }

    pub fn id(&self) -> &str {
        &self.agent_id
    }

    pub fn initialize_response(&self) -> schema::InitializeResponse {
        self.initialize.clone()
    }

    pub async fn session_id(&self) -> Option<String> {
        self.session_id.lock().await.clone()
    }

    /// Signal the supervisor to stop the agent process. No-op if the
    /// supervisor registration hasn't completed yet (extremely short
    /// window during `spawn`).
    pub async fn shutdown(&self) {
        tracing::info!("[{}-agent] shutdown signaled", self.agent_id);
        if let Some(id) = self.process_id.get() {
            if let Err(e) = Supervisor::global().force_stop(*id).await {
                tracing::info!(
                    "[{}-agent] supervisor force_stop failed: {}",
                    self.agent_id,
                    e
                );
            }
        }
    }
}

impl Agent {
    pub async fn initialize(
        &self,
        args: schema::InitializeRequest,
    ) -> acp::Result<schema::InitializeResponse> {
        self.conn.send_request(args).block_task().await
    }

    pub async fn authenticate(
        &self,
        args: schema::AuthenticateRequest,
    ) -> acp::Result<schema::AuthenticateResponse> {
        self.conn.send_request(args).block_task().await
    }

    pub async fn new_session(
        &self,
        args: schema::NewSessionRequest,
    ) -> acp::Result<schema::NewSessionResponse> {
        let resp = self.conn.send_request(args).block_task().await?;
        *self.session_id.lock().await = Some(resp.session_id.to_string());
        Ok(resp)
    }

    pub async fn load_session(
        &self,
        args: schema::LoadSessionRequest,
    ) -> acp::Result<schema::LoadSessionResponse> {
        let session_id = args.session_id.clone();
        let resp = self.conn.send_request(args).block_task().await?;
        *self.session_id.lock().await = Some(session_id.to_string());
        Ok(resp)
    }

    pub async fn set_session_mode(
        &self,
        args: schema::SetSessionModeRequest,
    ) -> acp::Result<schema::SetSessionModeResponse> {
        self.conn.send_request(args).block_task().await
    }

    pub async fn prompt(&self, args: schema::PromptRequest) -> acp::Result<schema::PromptResponse> {
        self.conn.send_request(args).block_task().await
    }

    pub async fn cancel(&self, args: schema::CancelNotification) -> acp::Result<()> {
        self.conn.send_notification(args)
    }

    pub async fn set_session_config_option(
        &self,
        args: schema::SetSessionConfigOptionRequest,
    ) -> acp::Result<schema::SetSessionConfigOptionResponse> {
        self.conn.send_request(args).block_task().await
    }
}

// ---------------------------------------------------------------------------
// Binary resolution
// ---------------------------------------------------------------------------

/// Resolve the agent's launch command, lazily installing the binary on
/// first miss. Returns `(program, args)` ready for a [`SpawnSpec`].
async fn resolve_agent_program(agent_id: &str) -> anyhow::Result<(String, Vec<String>)> {
    let agent_def = crate::resources::agent_by_id(agent_id)
        .ok_or_else(|| anyhow!("No resource definition for agent '{}'", agent_id))?;

    // 1. npm-based agents → `node <resolved_entry>`
    // 2. binary-download agents → install via install_cmd, run from PATH
    // 3. native agents → program + args from PATH
    if let Some(npm_pkg) = &agent_def.acp.npm_package {
        let default_bin_name = super::install::npm_package_bin_name(npm_pkg);
        let bin_name = agent_def
            .acp
            .bin_name
            .as_deref()
            .unwrap_or(&default_bin_name);
        if !super::install::npm_package_installed(npm_pkg, bin_name) {
            tracing::info!("[{}-agent] auto-installing {} ...", agent_id, npm_pkg);
            super::install::auto_install_npm_agent(npm_pkg).await?;
        }
        let entry = crate::process::env::resolve_acp_agent_bin(bin_name)
            .with_context(|| format!("Resolving ACP agent '{}' (npm: {})", agent_id, npm_pkg))?;
        Ok((
            "node".to_string(),
            vec![entry.to_string_lossy().to_string()],
        ))
    } else if let Some(install_cmd) = &agent_def.acp.install_cmd {
        if !super::install::is_program_available(&agent_def.acp.program) {
            tracing::info!("[{}-agent] auto-installing via install cmd ...", agent_id);
            super::install::auto_install_agent_cmd(install_cmd, agent_id).await?;
        }
        Ok((agent_def.acp.program.clone(), agent_def.acp.args.clone()))
    } else {
        Ok((agent_def.acp.program.clone(), agent_def.acp.args.clone()))
    }
}
