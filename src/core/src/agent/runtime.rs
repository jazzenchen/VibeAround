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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use anyhow::{anyhow, Context};
use tokio::sync::{oneshot, Mutex};

use acp::schema::v1 as schema;
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

/// Shared liveness token for one ACP process/connection generation.
///
/// The bridge owns one clone and marks it stopped when its IO future exits or
/// is cancelled. The workspace runtime consults the same token before reusing
/// an [`Agent`], so a dead connection cannot remain the active host forever.
#[derive(Clone)]
pub(crate) struct AcpSessionGeneration {
    live: Arc<AtomicBool>,
}

impl AcpSessionGeneration {
    pub(crate) fn running() -> Self {
        Self {
            live: Arc::new(AtomicBool::new(true)),
        }
    }

    pub(crate) fn is_live(&self) -> bool {
        self.live.load(Ordering::Acquire)
    }

    pub(crate) fn mark_stopped(&self) {
        self.live.store(false, Ordering::Release);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupSession {
    Fresh,
    /// Attach with startup replay. Web uses this to rebuild visible history.
    Load(String),
    /// Attach without startup replay. The bridge uses ACP `session/resume`
    /// where available, and otherwise suppresses startup notifications while
    /// falling back to `session/load`.
    Resume(String),
    /// Attach without replay and without falling back to `session/load`.
    ///
    /// Some agents write a fresh session record when asked to `session/load`;
    /// this mode avoids creating new sidebar entries when a user only opens an
    /// existing session.
    ResumeOnly(String),
}

impl StartupSession {
    pub fn session_id(&self) -> Option<&str> {
        match self {
            Self::Fresh => None,
            Self::Load(session_id) | Self::Resume(session_id) | Self::ResumeOnly(session_id) => {
                Some(session_id.as_str())
            }
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
    suppress_startup_notifications: Arc<AtomicBool>,
    generation: AcpSessionGeneration,
    /// Supervisor handle installed by [`Agent::spawn`] after registration.
    /// `None` until the registration returns — effectively
    /// a moment-of-initialization gap where `shutdown()` is a no-op.
    process_id: OnceLock<ProcessId>,
}

/// Owns a supervisor registration until it is transferred to [`Agent`].
///
/// `Agent::spawn` can be cancelled while the child is still initializing.
/// Dropping this guard unregisters that otherwise-unreachable process.
struct PendingProcessRegistration {
    supervisor: Arc<Supervisor>,
    process_id: Option<ProcessId>,
}

impl PendingProcessRegistration {
    fn new(supervisor: Arc<Supervisor>, process_id: ProcessId) -> Self {
        Self {
            supervisor,
            process_id: Some(process_id),
        }
    }

    fn transfer_to(mut self, agent: &Agent) {
        let process_id = self
            .process_id
            .take()
            .expect("pending process registration already transferred");
        agent
            .process_id
            .set(process_id)
            .expect("agent process registration already installed");
    }
}

impl Drop for PendingProcessRegistration {
    fn drop(&mut self) {
        let Some(process_id) = self.process_id.take() else {
            return;
        };
        let supervisor = Arc::clone(&self.supervisor);
        tokio::spawn(async move {
            if let Err(error) = supervisor.unregister(process_id).await {
                tracing::warn!(
                    process_id = %process_id,
                    error = %error,
                    "failed to clean cancelled agent registration"
                );
            }
        });
    }
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
        crate::resources::validate_acp_runtime_agent(&agent_id).map_err(anyhow::Error::msg)?;

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
        if let Some(path_env) = selected_agent_path_env(&agent_id) {
            spec = spec.env(crate::process::env::path_env_key(), path_env);
        }
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

        let supervisor = Supervisor::global();
        let id = supervisor.register(
            ProcessKind::AcpAgent,
            label,
            spec,
            RestartPolicy::Never,
            factory,
        );
        let registration = PendingProcessRegistration::new(supervisor, id);

        let err_label = agent_id.clone();
        let ready = ready_rx
            .await
            .map_err(|_| anyhow!("Agent bridge for {} died during init", err_label))??;

        // Transfer the only process owner after initialization succeeds.
        registration.transfer_to(&ready.agent);

        Ok(ready)
    }

    /// Constructor used by the bridge once the ACP handshake has succeeded.
    /// Not `pub` externally — only `agent::bridge` needs it.
    pub(crate) fn from_connection(
        conn: acp::ConnectionTo<acp::Agent>,
        agent_id: String,
        initialize: schema::InitializeResponse,
        startup_session_id: Option<String>,
        suppress_startup_notifications: Arc<AtomicBool>,
    ) -> Arc<Self> {
        Arc::new(Self {
            conn,
            agent_id,
            initialize,
            session_id: Mutex::new(startup_session_id),
            suppress_startup_notifications,
            generation: AcpSessionGeneration::running(),
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

    /// Whether this handle still belongs to the live ACP bridge generation.
    pub(crate) fn is_live(&self) -> bool {
        self.generation.is_live()
    }

    pub(crate) fn generation(&self) -> AcpSessionGeneration {
        self.generation.clone()
    }

    /// Signal the supervisor to stop the agent process. No-op if the
    /// supervisor registration hasn't completed yet (extremely short
    /// window during `spawn`).
    pub async fn shutdown(&self) {
        tracing::info!("[{}-agent] shutdown signaled", self.agent_id);
        self.generation.mark_stopped();
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

#[cfg(test)]
mod lifecycle_tests {
    use std::sync::Arc;

    use super::{AcpSessionGeneration, PendingProcessRegistration};
    use crate::process::bridge::{BridgeExit, BridgeFuture, ProcessBridge, StdioPipes};
    use crate::process::registry::{ChildRegistry, ProcessKind};
    use crate::process::supervisor::{RestartPolicy, SpawnSpec, Supervisor};

    struct HangingInitializeBridge;

    impl ProcessBridge for HangingInitializeBridge {
        fn run(
            self: Box<Self>,
            _pipes: StdioPipes,
            _cancel: crate::process::bridge::CancelSignal,
        ) -> BridgeFuture {
            Box::pin(async move {
                std::future::pending::<()>().await;
                BridgeExit::Cancelled
            })
        }
    }

    #[cfg(unix)]
    fn hanging_child_spec() -> SpawnSpec {
        SpawnSpec::new("sh").args(["-c", "while :; do sleep 1; done"])
    }

    #[cfg(windows)]
    fn hanging_child_spec() -> SpawnSpec {
        SpawnSpec::new("cmd").args(["/C", "ping -t 127.0.0.1 >NUL"])
    }

    #[test]
    fn session_generation_liveness_is_shared_across_owners() {
        let agent_generation = AcpSessionGeneration::running();
        let bridge_generation = agent_generation.clone();

        assert!(agent_generation.is_live());
        bridge_generation.mark_stopped();
        assert!(!agent_generation.is_live());
    }

    #[cfg(any(unix, windows))]
    #[tokio::test]
    async fn aborted_initialize_owner_unregisters_and_reaps_child() {
        let registry = Arc::new(ChildRegistry::new());
        let supervisor = Supervisor::new(Arc::clone(&registry));
        let (registered_tx, registered_rx) = tokio::sync::oneshot::channel();
        let task_supervisor = Arc::clone(&supervisor);
        let owner = tokio::spawn(async move {
            let process_id = task_supervisor.register(
                ProcessKind::AcpAgent,
                "hanging-agent-init",
                hanging_child_spec(),
                RestartPolicy::Never,
                Box::new(|| Box::new(HangingInitializeBridge)),
            );
            let _registration =
                PendingProcessRegistration::new(Arc::clone(&task_supervisor), process_id);
            let _ = registered_tx.send(process_id);
            std::future::pending::<()>().await;
        });
        let process_id = registered_rx.await.expect("registration published");

        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while registry.len() == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("child was not registered");

        owner.abort();
        let _ = owner.await;

        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                if registry.len() == 0
                    && !supervisor
                        .snapshot()
                        .iter()
                        .any(|process| process.id == process_id)
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("cancelled initialization leaked its process registration");
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
        self.allow_startup_notifications();
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
        self.allow_startup_notifications();
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

    fn allow_startup_notifications(&self) {
        self.suppress_startup_notifications
            .store(false, Ordering::SeqCst);
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
    let config = crate::config::ensure_loaded();
    let selected_candidate =
        resolve_agent_candidate(agent_id, config.toolchain_mode.as_str()).await;
    if config.toolchain_mode.is_managed() && selected_candidate.is_none() {
        anyhow::bail!(
            "{}",
            crate::agent_detection::managed_agent_missing_message(agent_id)
        );
    }

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
        if let Some(candidate) = selected_candidate {
            return Ok((candidate.path, agent_def.acp.args.clone()));
        }
        if !super::install::is_program_available(&agent_def.acp.program) {
            tracing::info!("[{}-agent] auto-installing via install cmd ...", agent_id);
            super::install::auto_install_agent_cmd(install_cmd, agent_id).await?;
        }
        Ok((agent_def.acp.program.clone(), agent_def.acp.args.clone()))
    } else {
        if let Some(candidate) = selected_candidate {
            return Ok((candidate.path, agent_def.acp.args.clone()));
        }
        Ok((agent_def.acp.program.clone(), agent_def.acp.args.clone()))
    }
}

async fn resolve_agent_candidate(
    agent_id: &str,
    toolchain_mode: &str,
) -> Option<crate::agent_detection::AgentCandidate> {
    crate::agent_availability::resolve_agent_availability(
        agent_id,
        crate::agent_availability::AgentAvailabilityRequest {
            scan_policy: crate::agent_availability::AgentScanPolicy::RefreshIfMissing,
            toolchain_mode,
            candidate_preference:
                crate::agent_availability::AgentCandidatePreference::ToolchainMode,
            include_configured_version: true,
        },
    )
    .await
    .ok()
    .and_then(|availability| availability.selected)
}

fn selected_agent_path_env(agent_id: &str) -> Option<String> {
    let candidate = crate::agent_detection::selected_candidate(agent_id)?;
    let parent = std::path::Path::new(&candidate.path).parent()?;
    let mut paths = vec![parent.to_path_buf()];
    if let Some(current) = crate::process::env::path_value(&crate::process::env::child_env()) {
        paths.extend(std::env::split_paths(&current));
    }
    std::env::join_paths(paths)
        .ok()
        .map(|value| value.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoopClientHandler;

    #[async_trait::async_trait]
    impl AgentClientHandler for NoopClientHandler {
        async fn session_notification(
            &self,
            _args: schema::SessionNotification,
        ) -> acp::Result<()> {
            Ok(())
        }

        async fn request_permission(
            &self,
            _args: schema::RequestPermissionRequest,
        ) -> acp::Result<schema::RequestPermissionResponse> {
            Ok(schema::RequestPermissionResponse::new(
                schema::RequestPermissionOutcome::Cancelled,
            ))
        }
    }

    #[tokio::test]
    async fn spawn_rejects_direct_only_agents_before_process_launch() {
        let workspace = std::env::temp_dir().join(format!(
            "vibearound-direct-only-agent-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&workspace).unwrap();

        let error = match Agent::spawn(
            "codex-desktop".to_string(),
            &RouteKey::new("qqbot", "chat-a"),
            &workspace,
            StartupSession::Fresh,
            Arc::new(NoopClientHandler),
            Vec::new(),
            Vec::new(),
        )
        .await
        {
            Ok(_) => panic!("direct-only agent should not spawn"),
            Err(error) => error,
        };

        assert!(format!("{:#}", error).contains("Codex Desktop can only be opened directly"));
        let _ = std::fs::remove_dir_all(&workspace);
    }
}
