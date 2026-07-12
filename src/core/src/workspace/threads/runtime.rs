//! Runtime owner for one workspace thread.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
use anyhow::Context;
use tokio::sync::{broadcast, mpsc, oneshot, watch, Mutex};
use tokio::time::{sleep, Duration};

use crate::agent::{Agent, AgentClientHandler, StartupSession};
use crate::routing::{channel_traits, wait_for_signal, ActiveTurnTarget, ChannelTarget, RouteKey};
use crate::workspace::registry::WorkspaceId;

use super::store::{
    HostBinding, MultiAgentTurn, ThreadAgent, ThreadAgentId, ThreadAgentStatus, ThreadEvent,
    ThreadEventStore, ThreadStatus, WorkspaceThread, WorkspaceThreadId,
};

#[derive(Debug, Clone)]
pub struct ThreadRuntimeState {
    pub thread_id: WorkspaceThreadId,
    pub workspace_id: WorkspaceId,
    pub host_binding: HostBinding,
    pub session_id: Option<String>,
    pub workspace: PathBuf,
    pub busy: bool,
    pub failed: Option<String>,
    pub initialize: Option<acp::InitializeResponse>,
    pub agents: Vec<ThreadAgent>,
    pub multi_agent_turns: Vec<MultiAgentTurn>,
}

pub(crate) struct PromptCancellation {
    cancel_rx: watch::Receiver<bool>,
    shutdown_rx: watch::Receiver<bool>,
}

#[derive(Clone, Default)]
struct TurnState {
    busy: bool,
    failed: Option<String>,
    session_id: Option<String>,
    host_agent: Option<Arc<Agent>>,
}

struct PromptCommand {
    runtime: Arc<ThreadRuntime>,
    target: ChannelTarget,
    content_blocks: Vec<acp::ContentBlock>,
    handler: Arc<dyn AgentClientHandler>,
    cancellation: Option<PromptCancellation>,
    reply: oneshot::Sender<acp::Result<acp::PromptResponse>>,
}

struct StartCommand {
    runtime: Arc<ThreadRuntime>,
    route: RouteKey,
    handler: Arc<dyn AgentClientHandler>,
    reply: oneshot::Sender<acp::Result<String>>,
}

struct RuntimeCommand<T> {
    runtime: Arc<ThreadRuntime>,
    reply: oneshot::Sender<T>,
}

struct CloseCommand {
    runtime: Arc<ThreadRuntime>,
    reason: Option<String>,
    reply: oneshot::Sender<acp::Result<()>>,
}

struct SwitchProfileCommand {
    runtime: Arc<ThreadRuntime>,
    host_binding: HostBinding,
    reply: oneshot::Sender<acp::Result<()>>,
}

enum ThreadOwnerCommand {
    Prompt(Box<PromptCommand>),
    Start(Box<StartCommand>),
    Cancel(oneshot::Sender<acp::Result<()>>),
    Close(Box<CloseCommand>),
    ShutdownHost(RuntimeCommand<()>),
    ShutdownHostIfIdle {
        runtime: Arc<ThreadRuntime>,
        generation: u64,
        reply: oneshot::Sender<bool>,
    },
    SwitchProfile(Box<SwitchProfileCommand>),
    #[cfg(test)]
    Probe {
        started: oneshot::Sender<()>,
        release: oneshot::Receiver<()>,
    },
}

struct ThreadOwner {
    command_rx: mpsc::UnboundedReceiver<ThreadOwnerCommand>,
    state_tx: watch::Sender<TurnState>,
    change_tx: Option<broadcast::Sender<()>>,
    host: Option<AcpSessionRunner>,
    session_id: Option<String>,
}

impl PromptCancellation {
    pub(crate) fn new(
        cancel_rx: watch::Receiver<bool>,
        shutdown_rx: watch::Receiver<bool>,
    ) -> Self {
        Self {
            cancel_rx,
            shutdown_rx,
        }
    }

    async fn cancelled(&mut self) {
        tokio::select! {
            _ = wait_for_signal(&mut self.cancel_rx) => {}
            _ = wait_for_signal(&mut self.shutdown_rx) => {}
        }
    }
}

impl ThreadOwner {
    async fn run(mut self) {
        while let Some(command) = self.command_rx.recv().await {
            match command {
                ThreadOwnerCommand::Prompt(command) => {
                    self.set_turn_state(true, None);
                    let PromptCommand {
                        runtime,
                        target,
                        content_blocks,
                        handler,
                        cancellation,
                        reply,
                    } = *command;
                    let result = self
                        .run_prompt(&runtime, &target, content_blocks, handler, cancellation)
                        .await;
                    self.set_turn_state(
                        false,
                        result.as_ref().err().map(|error| error.message.to_string()),
                    );
                    let _ = reply.send(result);
                }
                ThreadOwnerCommand::Start(command) => {
                    let StartCommand {
                        runtime,
                        route,
                        handler,
                        reply,
                    } = *command;
                    let result = self.start(&runtime, &route, handler).await;
                    let _ = reply.send(result);
                }
                ThreadOwnerCommand::Cancel(reply) => {
                    let result = self.cancel().await;
                    let _ = reply.send(result);
                }
                ThreadOwnerCommand::Close(command) => {
                    let CloseCommand {
                        runtime,
                        reason,
                        reply,
                    } = *command;
                    let result = self.close(&runtime, reason).await;
                    let _ = reply.send(result);
                }
                ThreadOwnerCommand::ShutdownHost(command) => {
                    self.shutdown_host_contents(&command.runtime).await;
                    let _ = command.reply.send(());
                }
                ThreadOwnerCommand::ShutdownHostIfIdle {
                    runtime,
                    generation,
                    reply,
                } => {
                    let stopped = self.shutdown_host_if_idle(&runtime, generation).await;
                    let _ = reply.send(stopped);
                }
                ThreadOwnerCommand::SwitchProfile(command) => {
                    let SwitchProfileCommand {
                        runtime,
                        host_binding,
                        reply,
                    } = *command;
                    let result = self.switch_profile(&runtime, host_binding).await;
                    let _ = reply.send(result);
                }
                #[cfg(test)]
                ThreadOwnerCommand::Probe { started, release } => {
                    self.set_turn_state(true, None);
                    let _ = started.send(());
                    let _ = release.await;
                    self.set_turn_state(false, None);
                }
            }
        }
    }

    async fn run_prompt(
        &mut self,
        runtime: &ThreadRuntime,
        target: &ChannelTarget,
        content_blocks: Vec<acp::ContentBlock>,
        handler: Arc<dyn AgentClientHandler>,
        mut route_cancellation: Option<PromptCancellation>,
    ) -> acp::Result<acp::PromptResponse> {
        let target_guard = runtime.active_turn_target.install(target.clone());
        let mut cancellation = target_guard.cancellation();
        let fallback_finish_handler = Arc::clone(&handler);

        let result = tokio::select! {
            biased;
            _ = wait_for_signal(&mut cancellation) => {
                Ok(acp::PromptResponse::new(acp::StopReason::Cancelled))
            }
            _ = wait_for_prompt_cancellation(&mut route_cancellation) => {
                Ok(acp::PromptResponse::new(acp::StopReason::Cancelled))
            }
            result = async {
                runtime.maybe_record_first_prompt(&content_blocks).await?;
                let agent = self.ensure_agent(runtime, &target.route, handler).await?;
                let session_id = self.ensure_session(runtime, &agent).await?;
                agent
                    .prompt(acp::PromptRequest::new(session_id, content_blocks))
                    .await
            } => result,
        };
        let finish_handler = self
            .host
            .as_ref()
            .map(|host| Arc::clone(&host.client_handler))
            .unwrap_or(fallback_finish_handler);
        if let Err(error) = finish_handler.prompt_finished(result.is_ok()).await {
            let thread_id = runtime.thread.lock().await.id.clone();
            tracing::warn!(
                thread_id = %thread_id,
                error = %error.message,
                "host prompt_finished hook failed"
            );
        }
        result
    }

    async fn start(
        &mut self,
        runtime: &ThreadRuntime,
        route: &RouteKey,
        handler: Arc<dyn AgentClientHandler>,
    ) -> acp::Result<String> {
        let agent = self.ensure_agent(runtime, route, handler).await?;
        self.ensure_session(runtime, &agent).await
    }

    async fn cancel(&mut self) -> acp::Result<()> {
        let agent = self
            .host
            .as_ref()
            .filter(|host| host.is_live())
            .map(|host| Arc::clone(&host.agent))
            .ok_or_else(acp::Error::method_not_found)?;
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(acp::Error::method_not_found)?;
        agent.cancel(acp::CancelNotification::new(session_id)).await
    }

    async fn close(&mut self, runtime: &ThreadRuntime, reason: Option<String>) -> acp::Result<()> {
        self.shutdown_host_contents(runtime).await;
        let thread_id = runtime.thread.lock().await.id.clone();
        let event = ThreadEvent::closed(thread_id, reason);
        append_thread_event(&runtime.store, &event).await?;
        runtime.apply_thread_event(&event).await?;
        self.session_id = None;
        self.publish_runtime_state();
        runtime.notify_change();
        Ok(())
    }

    async fn shutdown_host_contents(&mut self, runtime: &ThreadRuntime) {
        if let Some(session_id) = &self.session_id {
            crate::previews::kill_by_session(session_id);
        }
        if let Some(host) = self.host.take() {
            host.shutdown().await;
        }
        for (_, subagent) in std::mem::take(&mut *runtime.subagents.lock().await) {
            crate::previews::kill_by_session(&subagent.session_id);
            subagent.agent.shutdown().await;
        }
        let mut state = self.state_tx.borrow().clone();
        state.failed = None;
        self.set_state(state);
        self.publish_runtime_state();
        runtime.notify_change();
    }

    async fn shutdown_host_if_idle(&mut self, runtime: &ThreadRuntime, generation: u64) -> bool {
        if runtime.idle_generation() != generation
            || self.state_tx.borrow().busy
            || self.host.is_none()
            || !runtime.subagents.lock().await.is_empty()
            || runtime.idle_generation() != generation
        {
            return false;
        }
        self.shutdown_host_contents(runtime).await;
        true
    }

    async fn switch_profile(
        &mut self,
        runtime: &ThreadRuntime,
        host_binding: HostBinding,
    ) -> acp::Result<()> {
        {
            let thread = runtime.thread.lock().await;
            if thread.host_binding.agent_id != host_binding.agent_id {
                return Err(acp::Error::new(
                    -32602,
                    "profile switch cannot change agent",
                ));
            }
            if thread.host_binding == host_binding {
                return Ok(());
            }
        }

        let preserved_session_id = self.session_id.clone();
        if let Some(host) = self.host.take() {
            host.shutdown().await;
        }
        self.set_turn_state(false, None);

        let thread_id = runtime.thread.lock().await.id.clone();
        let event = ThreadEvent::host_changed(thread_id.clone(), host_binding.clone());
        append_thread_event(&runtime.store, &event).await?;
        runtime.apply_thread_event(&event).await?;

        if let Some(session_id) = preserved_session_id {
            let needs_session_ref = {
                let thread = runtime.thread.lock().await;
                !thread.has_agent_session(&host_binding, &session_id)
            };
            if needs_session_ref {
                let event = ThreadEvent::agent_session_observed(
                    thread_id,
                    host_binding.agent_id,
                    host_binding.profile_id,
                    session_id.clone(),
                );
                append_thread_event(&runtime.store, &event).await?;
                runtime.apply_thread_event(&event).await?;
            }
            self.session_id = Some(session_id);
        }
        self.publish_runtime_state();
        runtime.notify_change();
        Ok(())
    }

    async fn ensure_agent(
        &mut self,
        runtime: &ThreadRuntime,
        route: &RouteKey,
        handler: Arc<dyn AgentClientHandler>,
    ) -> acp::Result<Arc<Agent>> {
        if let Some(host) = self.host.as_ref().filter(|host| host.is_live()) {
            return Ok(Arc::clone(&host.agent));
        }
        if let Some(stale) = self.host.take() {
            let thread_id = runtime.thread.lock().await.id.clone();
            tracing::info!(
                thread_id = %thread_id,
                agent_id = %stale.agent.id(),
                "replacing stopped ACP host generation"
            );
            stale.shutdown().await;
        }

        let thread = runtime.thread.lock().await.clone();
        if thread.status == ThreadStatus::Closed {
            return Err(acp::Error::new(-32603, "workspace thread is closed"));
        }
        let agent_id = crate::resources::resolve_agent_id(&thread.host_binding.agent_id)
            .map_err(|error| acp::Error::new(-32602, error))?;
        let profile = thread
            .host_binding
            .profile_id
            .clone()
            .unwrap_or_else(|| "default".to_string());
        let startup_session = host_startup_session(route, self.session_id.clone(), &thread);

        std::fs::create_dir_all(&runtime.workspace).map_err(|error| {
            acp::Error::new(
                -32603,
                format!(
                    "failed to create workspace {:?}: {}",
                    runtime.workspace, error
                ),
            )
        })?;
        let mut env_vars = vec![
            (
                "VIBEAROUND_CHANNEL_KIND".to_string(),
                route.channel_kind.clone(),
            ),
            ("VIBEAROUND_CHAT_ID".to_string(), route.chat_id.clone()),
            ("VIBEAROUND_AGENT_KIND".to_string(), agent_id.clone()),
            ("VIBEAROUND_THREAD_ID".to_string(), thread.id.to_string()),
            (
                "VIBEAROUND_WORKSPACE_ID".to_string(),
                thread.workspace_id.to_string(),
            ),
        ];
        let mut extra_args = Vec::new();
        if crate::agent::launch::profile_uses_vibearound_credentials(&profile) {
            let applied = crate::agent::launch::materialize_profile_for_agent(
                &profile,
                &agent_id,
                &runtime.workspace,
                route,
            )
            .map_err(|error| acp::Error::new(-32603, format!("{:#}", error)))?;
            env_vars.extend(applied.env);
            extra_args.extend(applied.command_args);
        }
        crate::agent::launch::append_profile_id_env(
            &mut env_vars,
            thread.host_binding.profile_id.as_deref(),
        );
        let agent_prefs = crate::agent_state::read_prefs();
        extra_args.extend(crate::agent_state::resolve_agent_acp_args(
            &agent_prefs,
            &agent_id,
        ));

        let spawned_handler = Arc::clone(&handler);
        let ready = Agent::spawn(
            agent_id.clone(),
            route,
            &runtime.workspace,
            startup_session.clone(),
            handler,
            extra_args,
            env_vars,
        )
        .await
        .map_err(|error| acp::Error::new(-32603, format!("{:#}", error)))?;

        if runtime.thread.lock().await.status == ThreadStatus::Closed {
            ready.agent.shutdown().await;
            return Err(acp::Error::new(-32603, "workspace thread is closed"));
        }
        self.host = Some(AcpSessionRunner {
            agent: Arc::clone(&ready.agent),
            client_handler: spawned_handler,
        });
        self.publish_runtime_state();

        if let Some(session_id) = ready.startup_session_id {
            self.observe_session(
                runtime,
                &agent_id,
                thread.host_binding.profile_id,
                &session_id,
            )
            .await?;
        } else if startup_session.session_id().is_some() {
            self.session_id = None;
            self.publish_runtime_state();
        }
        Ok(ready.agent)
    }

    async fn ensure_session(
        &mut self,
        runtime: &ThreadRuntime,
        agent: &Arc<Agent>,
    ) -> acp::Result<String> {
        if let Some(session_id) = &self.session_id {
            return Ok(session_id.clone());
        }
        let response = agent
            .new_session(acp::NewSessionRequest::new(runtime.workspace.clone()))
            .await?;
        let session_id = response.session_id.to_string();
        let host = runtime.thread.lock().await.host_binding.clone();
        self.observe_session(runtime, &host.agent_id, host.profile_id, &session_id)
            .await?;
        Ok(session_id)
    }

    async fn observe_session(
        &mut self,
        runtime: &ThreadRuntime,
        agent_id: &str,
        profile_id: Option<String>,
        session_id: &str,
    ) -> acp::Result<()> {
        if self.session_id.as_deref() == Some(session_id) {
            return Ok(());
        }
        let binding = HostBinding::new(agent_id.to_string(), profile_id.clone());
        if runtime
            .thread
            .lock()
            .await
            .has_agent_session(&binding, session_id)
        {
            self.session_id = Some(session_id.to_string());
            self.publish_runtime_state();
            return Ok(());
        }
        let thread_id = runtime.thread.lock().await.id.clone();
        let event = ThreadEvent::agent_session_observed(
            thread_id,
            agent_id.to_string(),
            profile_id,
            session_id.to_string(),
        );
        append_thread_event(&runtime.store, &event).await?;
        runtime.apply_thread_event(&event).await?;
        self.session_id = Some(session_id.to_string());
        self.publish_runtime_state();
        runtime.notify_change();
        Ok(())
    }

    fn set_state(&self, state: TurnState) {
        self.state_tx.send_replace(state);
        if let Some(change_tx) = &self.change_tx {
            let _ = change_tx.send(());
        }
    }

    fn set_turn_state(&self, busy: bool, failed: Option<String>) {
        let mut state = self.state_tx.borrow().clone();
        state.busy = busy;
        state.failed = failed;
        self.set_state(state);
    }

    fn publish_runtime_state(&self) {
        let mut state = self.state_tx.borrow().clone();
        state.session_id = self.session_id.clone();
        state.host_agent = self.host.as_ref().map(|host| Arc::clone(&host.agent));
        self.set_state(state);
    }
}

#[derive(Clone)]
struct SubagentRuntime {
    agent: Arc<Agent>,
    session_id: String,
    client_handler: Arc<dyn AgentClientHandler>,
    active_turn_target: ActiveTurnTarget,
    completion_validator: Option<Arc<dyn SubagentCompletionValidator>>,
}

/// One live host-agent generation owned by a workspace thread.
///
/// The ACP session id remains on `ThreadRuntime` because it is durable across
/// generations. Everything tied to one connection/process is replaced as a
/// unit when the bridge reports that generation dead.
struct AcpSessionRunner {
    agent: Arc<Agent>,
    client_handler: Arc<dyn AgentClientHandler>,
}

impl AcpSessionRunner {
    fn is_live(&self) -> bool {
        self.agent.is_live()
    }

    async fn shutdown(self) {
        self.agent.shutdown().await;
    }
}

#[derive(Debug, Clone)]
pub struct SubagentCompletionResult {
    pub status: ThreadAgentStatus,
    pub last_error: Option<String>,
    pub report: Option<serde_json::Value>,
}

#[async_trait::async_trait]
pub trait SubagentCompletionValidator: Send + Sync + 'static {
    async fn reset_completion(&self);

    async fn validate_completion(&self) -> Result<SubagentCompletionResult, String>;
}

const SUBAGENT_START_MAX_ATTEMPTS: usize = 2;
const SUBAGENT_PROMPT_MAX_ATTEMPTS: usize = 2;
const SUBAGENT_RETRY_DELAY: Duration = Duration::from_millis(750);

pub struct ThreadRuntime {
    thread: Mutex<WorkspaceThread>,
    workspace: PathBuf,
    subagents: Mutex<BTreeMap<ThreadAgentId, SubagentRuntime>>,
    active_turn_target: ActiveTurnTarget,
    owner_tx: mpsc::UnboundedSender<ThreadOwnerCommand>,
    turn_state: watch::Receiver<TurnState>,
    activity_generation: AtomicU64,
    store: ThreadEventStore,
    change_tx: Option<broadcast::Sender<()>>,
}

impl ThreadRuntime {
    pub fn new(thread: WorkspaceThread, workspace: PathBuf, store: ThreadEventStore) -> Self {
        Self::with_change_tx(thread, workspace, store, None)
    }

    pub fn with_change_tx(
        thread: WorkspaceThread,
        workspace: PathBuf,
        store: ThreadEventStore,
        change_tx: Option<broadcast::Sender<()>>,
    ) -> Self {
        let session_id = latest_session_for_host(&thread);
        let (owner_tx, owner_rx) = mpsc::unbounded_channel();
        let (turn_state_tx, turn_state) = watch::channel(TurnState {
            session_id: session_id.clone(),
            ..TurnState::default()
        });
        tokio::spawn(
            ThreadOwner {
                command_rx: owner_rx,
                state_tx: turn_state_tx,
                change_tx: change_tx.clone(),
                host: None,
                session_id,
            }
            .run(),
        );
        Self {
            thread: Mutex::new(thread),
            workspace,
            subagents: Mutex::new(BTreeMap::new()),
            active_turn_target: ActiveTurnTarget::default(),
            owner_tx,
            turn_state,
            activity_generation: AtomicU64::new(0),
            store,
            change_tx,
        }
    }

    pub async fn state(&self) -> ThreadRuntimeState {
        let thread = self.thread.lock().await.clone();
        let turn_state = self.turn_state.borrow().clone();
        let initialize = turn_state
            .host_agent
            .as_ref()
            .filter(|agent| agent.is_live())
            .map(|agent| agent.initialize_response());
        ThreadRuntimeState {
            thread_id: thread.id.clone(),
            workspace_id: thread.workspace_id.clone(),
            host_binding: thread.host_binding.clone(),
            session_id: turn_state.session_id,
            workspace: self.workspace.clone(),
            busy: turn_state.busy,
            failed: turn_state.failed,
            initialize,
            agents: thread.agents.values().cloned().collect(),
            multi_agent_turns: thread.multi_agent_turns.values().cloned().collect(),
        }
    }

    pub fn active_turn_target(&self) -> ActiveTurnTarget {
        self.active_turn_target.clone()
    }

    /// Start the host agent and ensure a session exists, without sending a
    /// user prompt. This backs `/new` and route attachment warmup.
    pub async fn start(
        self: &Arc<Self>,
        route: &RouteKey,
        handler: Arc<dyn AgentClientHandler>,
    ) -> acp::Result<String> {
        self.mark_activity();
        let (reply, done) = oneshot::channel();
        self.owner_tx
            .send(ThreadOwnerCommand::Start(Box::new(StartCommand {
                runtime: Arc::clone(self),
                route: route.clone(),
                handler,
                reply,
            })))
            .map_err(|_| runtime_stopped_error())?;
        done.await.unwrap_or_else(|_| Err(runtime_stopped_error()))
    }

    pub async fn prompt(
        self: &Arc<Self>,
        target: &ChannelTarget,
        content_blocks: Vec<acp::ContentBlock>,
        handler: Arc<dyn AgentClientHandler>,
    ) -> acp::Result<acp::PromptResponse> {
        self.enqueue_prompt(target, content_blocks, handler, None)
            .await
    }

    pub(crate) async fn prompt_cancellable(
        self: &Arc<Self>,
        target: &ChannelTarget,
        content_blocks: Vec<acp::ContentBlock>,
        handler: Arc<dyn AgentClientHandler>,
        cancellation: PromptCancellation,
    ) -> acp::Result<acp::PromptResponse> {
        self.enqueue_prompt(target, content_blocks, handler, Some(cancellation))
            .await
    }

    async fn enqueue_prompt(
        self: &Arc<Self>,
        target: &ChannelTarget,
        content_blocks: Vec<acp::ContentBlock>,
        handler: Arc<dyn AgentClientHandler>,
        cancellation: Option<PromptCancellation>,
    ) -> acp::Result<acp::PromptResponse> {
        self.mark_activity();
        let (reply, done) = oneshot::channel();
        self.owner_tx
            .send(ThreadOwnerCommand::Prompt(Box::new(PromptCommand {
                runtime: Arc::clone(self),
                target: target.clone(),
                content_blocks,
                handler,
                cancellation,
                reply,
            })))
            .map_err(|_| acp::Error::new(-32603, "thread runtime stopped"))?;
        done.await
            .unwrap_or_else(|_| Err(acp::Error::new(-32603, "thread runtime stopped")))
    }

    pub async fn cancel(self: &Arc<Self>) -> acp::Result<()> {
        self.mark_activity();
        self.active_turn_target.cancel_current();
        {
            let subagents = self.subagents.lock().await;
            for subagent in subagents.values() {
                subagent.active_turn_target.cancel_current();
            }
        }
        let (reply, done) = oneshot::channel();
        self.owner_tx
            .send(ThreadOwnerCommand::Cancel(reply))
            .map_err(|_| runtime_stopped_error())?;
        done.await.unwrap_or_else(|_| Err(runtime_stopped_error()))
    }

    pub async fn close(self: &Arc<Self>, reason: Option<String>) -> acp::Result<()> {
        self.mark_activity();
        self.active_turn_target.cancel_current();
        let (reply, done) = oneshot::channel();
        self.owner_tx
            .send(ThreadOwnerCommand::Close(Box::new(CloseCommand {
                runtime: Arc::clone(self),
                reason,
                reply,
            })))
            .map_err(|_| runtime_stopped_error())?;
        done.await.unwrap_or_else(|_| Err(runtime_stopped_error()))
    }

    pub async fn shutdown_host(self: &Arc<Self>) {
        self.mark_activity();
        self.active_turn_target.cancel_current();
        let (reply, done) = oneshot::channel();
        if self
            .owner_tx
            .send(ThreadOwnerCommand::ShutdownHost(RuntimeCommand {
                runtime: Arc::clone(self),
                reply,
            }))
            .is_ok()
        {
            let _ = done.await;
        }
    }

    pub fn idle_generation(&self) -> u64 {
        self.activity_generation.load(Ordering::Relaxed)
    }

    pub async fn shutdown_host_if_idle(self: &Arc<Self>, generation: u64) -> bool {
        let (reply, done) = oneshot::channel();
        if self
            .owner_tx
            .send(ThreadOwnerCommand::ShutdownHostIfIdle {
                runtime: Arc::clone(self),
                generation,
                reply,
            })
            .is_err()
        {
            return false;
        }
        done.await.unwrap_or(false)
    }

    pub async fn switch_profile_preserving_session(
        self: &Arc<Self>,
        host_binding: HostBinding,
    ) -> acp::Result<()> {
        self.mark_activity();
        let (reply, done) = oneshot::channel();
        self.owner_tx
            .send(ThreadOwnerCommand::SwitchProfile(Box::new(
                SwitchProfileCommand {
                    runtime: Arc::clone(self),
                    host_binding,
                    reply,
                },
            )))
            .map_err(|_| runtime_stopped_error())?;
        done.await.unwrap_or_else(|_| Err(runtime_stopped_error()))
    }

    pub async fn initialize_multi_agent_turn(
        &self,
        turn: MultiAgentTurn,
        agents: Vec<ThreadAgent>,
    ) -> acp::Result<()> {
        self.mark_activity();
        if self.thread.lock().await.status == ThreadStatus::Closed {
            return Err(acp::Error::new(-32603, "workspace thread is closed"));
        }

        let thread_id = self.thread.lock().await.id.clone();
        let event = ThreadEvent::multi_agent_turn_initialized(thread_id, turn, agents);
        append_thread_event(&self.store, &event).await?;
        self.apply_thread_event(&event).await?;
        self.notify_change();
        Ok(())
    }

    pub async fn recover_interrupted_subagents(&self) -> acp::Result<Vec<ThreadAgent>> {
        let interrupted_ids = {
            let thread = self.thread.lock().await;
            if thread.status == ThreadStatus::Closed {
                return Ok(Vec::new());
            }
            thread
                .agents
                .values()
                .filter(|agent| agent.status == ThreadAgentStatus::Running)
                .map(|agent| agent.id.clone())
                .collect::<Vec<_>>()
        };

        let mut recovered = Vec::with_capacity(interrupted_ids.len());
        for agent_id in interrupted_ids {
            if let Some(updated) = self
                .set_thread_agent_status(
                    &agent_id,
                    ThreadAgentStatus::Error,
                    Some(
                        "Subagent process was interrupted before it reported completion."
                            .to_string(),
                    ),
                    None,
                )
                .await?
            {
                recovered.push(updated);
            }
        }
        Ok(recovered)
    }

    pub async fn start_subagent_assignment(
        self: &Arc<Self>,
        target: ChannelTarget,
        thread_agent: ThreadAgent,
        handler: Arc<dyn AgentClientHandler>,
        active_turn_target: ActiveTurnTarget,
        status_tx: mpsc::UnboundedSender<ThreadAgent>,
        completion_validator: Option<Arc<dyn SubagentCompletionValidator>>,
    ) -> acp::Result<()> {
        self.mark_activity();
        if self.thread.lock().await.status == ThreadStatus::Closed {
            return Err(acp::Error::new(-32603, "workspace thread is closed"));
        }

        let runtime_handler = Arc::clone(&handler);
        let (agent, session_id) = match self
            .spawn_subagent_session_with_retries(&target.route, &thread_agent, handler)
            .await
        {
            Ok(session) => session,
            Err(error) => {
                if let Ok(Some(updated)) = self
                    .set_thread_agent_status(
                        &thread_agent.id,
                        ThreadAgentStatus::Error,
                        Some(error.message.to_string()),
                        None,
                    )
                    .await
                {
                    let _ = status_tx.send(updated);
                }
                return Err(error);
            }
        };
        let completion_validator_for_runtime = completion_validator.clone();
        self.subagents.lock().await.insert(
            thread_agent.id.clone(),
            SubagentRuntime {
                agent: Arc::clone(&agent),
                session_id: session_id.clone(),
                client_handler: Arc::clone(&runtime_handler),
                active_turn_target: active_turn_target.clone(),
                completion_validator: completion_validator_for_runtime,
            },
        );

        if let Some(updated) = self
            .set_thread_agent_status_with_session(
                &thread_agent.id,
                ThreadAgentStatus::Running,
                Some(session_id.clone()),
                None,
                None,
            )
            .await?
        {
            let _ = status_tx.send(updated);
        }

        let prompt = subagent_assignment_prompt(&thread_agent);
        self.spawn_subagent_prompt_task(
            thread_agent,
            agent,
            session_id,
            prompt,
            target,
            active_turn_target,
            status_tx,
            runtime_handler,
            completion_validator,
        );

        Ok(())
    }

    pub async fn prompt_subagent_assignment(
        self: &Arc<Self>,
        agent_id: &ThreadAgentId,
        assignment: serde_json::Value,
        target: ChannelTarget,
        status_tx: mpsc::UnboundedSender<ThreadAgent>,
    ) -> acp::Result<()> {
        self.mark_activity();
        if self.thread.lock().await.status == ThreadStatus::Closed {
            return Err(acp::Error::new(-32603, "workspace thread is closed"));
        }

        let thread_agent = {
            let thread = self.thread.lock().await;
            let agent = thread
                .agents
                .get(agent_id)
                .ok_or_else(|| acp::Error::new(-32602, "subagent not found"))?;
            if agent.status == ThreadAgentStatus::Running {
                return Err(acp::Error::new(-32603, "subagent is already running"));
            }
            agent.clone()
        };
        validate_subagent_assignment(&thread_agent, agent_id, &assignment)?;

        let Some(subagent) = self.subagents.lock().await.get(agent_id).cloned() else {
            if let Ok(Some(updated)) = self
                .set_thread_agent_status(
                    agent_id,
                    ThreadAgentStatus::Error,
                    Some("Subagent process is not available in this host runtime.".to_string()),
                    None,
                )
                .await
            {
                let _ = status_tx.send(updated);
            }
            return Err(acp::Error::new(
                -32603,
                "subagent process is not available in this host runtime",
            ));
        };

        if let Some(updated) = self
            .set_thread_agent_status(agent_id, ThreadAgentStatus::Running, None, None)
            .await?
        {
            let _ = status_tx.send(updated);
        }

        let prompt = subagent_assignment_prompt_from_value(&thread_agent, &assignment);
        self.spawn_subagent_prompt_task(
            thread_agent,
            subagent.agent,
            subagent.session_id,
            prompt,
            target,
            subagent.active_turn_target,
            status_tx,
            subagent.client_handler,
            subagent.completion_validator,
        );

        Ok(())
    }

    async fn spawn_subagent_session_with_retries(
        &self,
        route: &RouteKey,
        thread_agent: &ThreadAgent,
        handler: Arc<dyn AgentClientHandler>,
    ) -> acp::Result<(Arc<Agent>, String)> {
        let mut last_error = None;
        for attempt in 1..=SUBAGENT_START_MAX_ATTEMPTS {
            let agent = match self
                .spawn_subagent(route, thread_agent, Arc::clone(&handler))
                .await
            {
                Ok(agent) => agent,
                Err(error) => {
                    last_error = Some(error.message.to_string());
                    if attempt < SUBAGENT_START_MAX_ATTEMPTS {
                        sleep(SUBAGENT_RETRY_DELAY).await;
                        continue;
                    }
                    break;
                }
            };

            match agent
                .new_session(subagent_new_session_request(thread_agent))
                .await
            {
                Ok(session) => return Ok((agent, session.session_id.to_string())),
                Err(error) => {
                    last_error = Some(error.message.to_string());
                    agent.shutdown().await;
                    if attempt < SUBAGENT_START_MAX_ATTEMPTS {
                        sleep(SUBAGENT_RETRY_DELAY).await;
                    }
                }
            }
        }

        Err(acp::Error::new(
            -32603,
            last_error.unwrap_or_else(|| "failed to start subagent session".to_string()),
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_subagent_prompt_task(
        self: &Arc<Self>,
        thread_agent: ThreadAgent,
        agent: Arc<Agent>,
        session_id: String,
        prompt: String,
        target: ChannelTarget,
        active_turn_target: ActiveTurnTarget,
        status_tx: mpsc::UnboundedSender<ThreadAgent>,
        prompt_finish_handler: Arc<dyn AgentClientHandler>,
        completion_validator: Option<Arc<dyn SubagentCompletionValidator>>,
    ) {
        let runtime = Arc::clone(self);
        tokio::spawn(async move {
            runtime
                .run_subagent_prompt_with_retries(
                    thread_agent,
                    agent,
                    session_id,
                    prompt,
                    target,
                    active_turn_target,
                    status_tx,
                    prompt_finish_handler,
                    completion_validator,
                )
                .await;
        });
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_subagent_prompt_with_retries(
        self: Arc<Self>,
        thread_agent: ThreadAgent,
        agent: Arc<Agent>,
        session_id: String,
        mut prompt: String,
        target: ChannelTarget,
        active_turn_target: ActiveTurnTarget,
        status_tx: mpsc::UnboundedSender<ThreadAgent>,
        prompt_finish_handler: Arc<dyn AgentClientHandler>,
        completion_validator: Option<Arc<dyn SubagentCompletionValidator>>,
    ) {
        let _target_guard = active_turn_target.install(target);
        let agent_id = thread_agent.id.clone();
        for attempt in 1..=SUBAGENT_PROMPT_MAX_ATTEMPTS {
            if let Some(validator) = completion_validator.as_ref() {
                validator.reset_completion().await;
            }

            let result = agent
                .prompt(acp::PromptRequest::new(
                    session_id.clone(),
                    vec![acp::ContentBlock::Text(acp::TextContent::new(
                        prompt.clone(),
                    ))],
                ))
                .await;
            if let Err(error) = prompt_finish_handler.prompt_finished(result.is_ok()).await {
                tracing::warn!(
                    agent_id = %agent_id,
                    error = %error.message,
                    "subagent prompt_finished hook failed"
                );
            }

            match result {
                Ok(_) => match completion_validator.as_ref() {
                    Some(validator) => match validator.validate_completion().await {
                        Ok(completion) => {
                            self.set_subagent_completion(&agent_id, completion, &status_tx)
                                .await;
                            return;
                        }
                        Err(message) => {
                            if attempt < SUBAGENT_PROMPT_MAX_ATTEMPTS {
                                tracing::info!(
                                    agent_id = %agent_id,
                                    error = %message,
                                    "retrying subagent completion report"
                                );
                                prompt = subagent_report_repair_prompt(&thread_agent, &message);
                                sleep(SUBAGENT_RETRY_DELAY).await;
                                continue;
                            }
                            self.set_subagent_error(&agent_id, message, &status_tx)
                                .await;
                            return;
                        }
                    },
                    None => {
                        self.set_subagent_completion(
                            &agent_id,
                            SubagentCompletionResult {
                                status: ThreadAgentStatus::Completed,
                                last_error: None,
                                report: None,
                            },
                            &status_tx,
                        )
                        .await;
                        return;
                    }
                },
                Err(error) => {
                    let message = error.message.to_string();
                    if attempt < SUBAGENT_PROMPT_MAX_ATTEMPTS {
                        tracing::info!(
                            agent_id = %agent_id,
                            error = %message,
                            "retrying subagent prompt after error"
                        );
                        sleep(SUBAGENT_RETRY_DELAY).await;
                        continue;
                    }
                    self.set_subagent_error(&agent_id, message, &status_tx)
                        .await;
                    return;
                }
            }
        }
    }

    async fn set_subagent_completion(
        &self,
        agent_id: &ThreadAgentId,
        completion: SubagentCompletionResult,
        status_tx: &mpsc::UnboundedSender<ThreadAgent>,
    ) {
        self.set_subagent_status_and_notify(
            agent_id,
            completion.status,
            completion.last_error,
            completion.report,
            status_tx,
        )
        .await;
    }

    async fn set_subagent_error(
        &self,
        agent_id: &ThreadAgentId,
        message: String,
        status_tx: &mpsc::UnboundedSender<ThreadAgent>,
    ) {
        self.set_subagent_status_and_notify(
            agent_id,
            ThreadAgentStatus::Error,
            Some(message),
            None,
            status_tx,
        )
        .await;
    }

    async fn set_subagent_status_and_notify(
        &self,
        agent_id: &ThreadAgentId,
        status: ThreadAgentStatus,
        last_error: Option<String>,
        report: Option<serde_json::Value>,
        status_tx: &mpsc::UnboundedSender<ThreadAgent>,
    ) {
        match self
            .set_thread_agent_status(agent_id, status, last_error, report)
            .await
        {
            Ok(Some(updated)) => {
                let _ = status_tx.send(updated);
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(
                    agent_id = %agent_id,
                    error = %error.message,
                    "failed to update subagent status"
                );
            }
        }
    }

    async fn spawn_subagent(
        &self,
        route: &RouteKey,
        thread_agent: &ThreadAgent,
        handler: Arc<dyn AgentClientHandler>,
    ) -> acp::Result<Arc<Agent>> {
        self.spawn_subagent_agent(route, thread_agent, handler, None)
            .await
    }

    pub async fn replay_subagent_session(
        &self,
        route: &RouteKey,
        thread_agent: &ThreadAgent,
        session_id: String,
        handler: Arc<dyn AgentClientHandler>,
    ) -> acp::Result<()> {
        let agent = self
            .spawn_subagent_agent(route, thread_agent, handler, Some(session_id))
            .await?;
        sleep(Duration::from_millis(250)).await;
        agent.shutdown().await;
        Ok(())
    }

    async fn spawn_subagent_agent(
        &self,
        route: &RouteKey,
        thread_agent: &ThreadAgent,
        handler: Arc<dyn AgentClientHandler>,
        resume_session_id: Option<String>,
    ) -> acp::Result<Arc<Agent>> {
        let thread = self.thread.lock().await.clone();
        let agent_id = crate::resources::resolve_agent_id(&thread_agent.agent_id)
            .map_err(|error| acp::Error::new(-32602, error))?;
        let profile = thread_agent
            .profile_id
            .clone()
            .unwrap_or_else(|| "default".to_string());
        let worktree = PathBuf::from(&thread_agent.worktree);
        std::fs::create_dir_all(&worktree).map_err(|error| {
            acp::Error::new(
                -32603,
                format!(
                    "failed to create subagent worktree {:?}: {}",
                    worktree, error
                ),
            )
        })?;

        let mut env_vars = vec![
            (
                "VIBEAROUND_CHANNEL_KIND".to_string(),
                route.channel_kind.clone(),
            ),
            ("VIBEAROUND_CHAT_ID".to_string(), route.chat_id.clone()),
            ("VIBEAROUND_AGENT_KIND".to_string(), agent_id.clone()),
            ("VIBEAROUND_AGENT_ROLE".to_string(), "subagent".to_string()),
            ("VIBEAROUND_THREAD_ID".to_string(), thread.id.to_string()),
            (
                "VIBEAROUND_WORKSPACE_ID".to_string(),
                thread.workspace_id.to_string(),
            ),
            (
                "VIBEAROUND_SUBAGENT_ID".to_string(),
                thread_agent.id.to_string(),
            ),
            (
                "VIBEAROUND_SUBAGENT_NAME".to_string(),
                thread_agent.name.clone(),
            ),
            (
                "VIBEAROUND_MULTI_AGENT_TURN_ID".to_string(),
                thread_agent.turn_id.to_string(),
            ),
        ];
        let mut extra_args = Vec::new();
        if crate::agent::launch::profile_uses_vibearound_credentials(&profile) {
            let applied = crate::agent::launch::materialize_profile_for_agent(
                &profile, &agent_id, &worktree, route,
            )
            .map_err(|error| acp::Error::new(-32603, format!("{:#}", error)))?;
            env_vars.extend(applied.env);
            extra_args.extend(applied.command_args);
        }
        crate::agent::launch::append_profile_id_env(
            &mut env_vars,
            thread_agent.profile_id.as_deref(),
        );
        let agent_prefs = crate::agent_state::read_prefs();
        extra_args.extend(crate::agent_state::resolve_agent_acp_args(
            &agent_prefs,
            &agent_id,
        ));

        let ready = Agent::spawn(
            agent_id,
            route,
            &worktree,
            resume_session_id
                .map(StartupSession::Load)
                .unwrap_or(StartupSession::Fresh),
            handler,
            extra_args,
            env_vars,
        )
        .await
        .map_err(|error| acp::Error::new(-32603, format!("{:#}", error)))?;
        Ok(ready.agent)
    }

    async fn set_thread_agent_status(
        &self,
        agent_id: &ThreadAgentId,
        status: ThreadAgentStatus,
        last_error: Option<String>,
        report: Option<serde_json::Value>,
    ) -> acp::Result<Option<ThreadAgent>> {
        self.set_thread_agent_status_with_session(agent_id, status, None, last_error, report)
            .await
    }

    async fn set_thread_agent_status_with_session(
        &self,
        agent_id: &ThreadAgentId,
        status: ThreadAgentStatus,
        session_id: Option<String>,
        last_error: Option<String>,
        report: Option<serde_json::Value>,
    ) -> acp::Result<Option<ThreadAgent>> {
        let thread_id = self.thread.lock().await.id.clone();
        let event = ThreadEvent::thread_agent_status_changed_with_session(
            thread_id,
            agent_id.clone(),
            status,
            session_id,
            last_error,
            report,
        );
        append_thread_event(&self.store, &event).await?;
        self.apply_thread_event(&event).await?;
        self.notify_change();
        let thread = self.thread.lock().await;
        Ok(thread.agents.get(agent_id).cloned())
    }

    async fn maybe_record_first_prompt(
        &self,
        content_blocks: &[acp::ContentBlock],
    ) -> acp::Result<()> {
        if self.thread.lock().await.first_user_prompt.is_some() {
            return Ok(());
        }
        let Some(prompt) = first_text(content_blocks) else {
            return Ok(());
        };
        let thread_id = self.thread.lock().await.id.clone();
        let event = ThreadEvent::first_user_prompt_set(thread_id, prompt);
        append_thread_event(&self.store, &event).await?;
        let result = self.apply_thread_event(&event).await;
        self.notify_change();
        result
    }

    async fn apply_thread_event(&self, event: &ThreadEvent) -> acp::Result<()> {
        let mut thread = self.thread.lock().await;
        match event {
            ThreadEvent::FirstUserPromptSet {
                occurred_at,
                prompt,
                ..
            } => {
                if thread.first_user_prompt.is_none() {
                    thread.first_user_prompt = Some(prompt.clone());
                }
                thread.updated_at = occurred_at.clone();
            }
            ThreadEvent::HostChanged {
                occurred_at,
                host_binding,
                ..
            } => {
                thread.host_binding = host_binding.clone();
                thread.updated_at = occurred_at.clone();
            }
            ThreadEvent::AgentSessionObserved {
                occurred_at,
                agent_id,
                profile_id,
                session_id,
                ..
            } => {
                let session = super::store::AgentSessionRef {
                    agent_id: agent_id.clone(),
                    profile_id: profile_id.clone(),
                    session_id: session_id.clone(),
                    observed_at: occurred_at.clone(),
                };
                if thread.has_agent_session(&session.binding(), &session.session_id) {
                    return Ok(());
                }
                thread
                    .agent_sessions
                    .entry(session.binding())
                    .or_default()
                    .push(session);
                thread.updated_at = occurred_at.clone();
            }
            ThreadEvent::MultiAgentTurnInitialized {
                occurred_at,
                turn,
                agents,
                ..
            } => {
                thread
                    .multi_agent_turns
                    .insert(turn.id.clone(), turn.clone());
                for agent in agents {
                    thread.agents.insert(agent.id.clone(), agent.clone());
                }
                thread.updated_at = occurred_at.clone();
            }
            ThreadEvent::ThreadAgentStatusChanged {
                occurred_at,
                agent_id,
                status,
                session_id,
                last_error,
                report,
                ..
            } => {
                let turn_id = if let Some(agent) = thread.agents.get_mut(agent_id) {
                    agent.status = *status;
                    if session_id.is_some() {
                        agent.session_id = session_id.clone();
                    }
                    agent.last_error = last_error.clone();
                    agent.report = report.clone();
                    agent.updated_at = occurred_at.clone();
                    Some(agent.turn_id.clone())
                } else {
                    None
                };
                if let Some(turn_id) = turn_id {
                    if let Some(agent_ids) = thread
                        .multi_agent_turns
                        .get(&turn_id)
                        .map(|turn| turn.agent_ids.clone())
                    {
                        let status = aggregate_turn_status(&agent_ids, &thread.agents);
                        if let Some(turn) = thread.multi_agent_turns.get_mut(&turn_id) {
                            turn.status = status;
                            turn.updated_at = occurred_at.clone();
                        }
                    }
                }
                thread.updated_at = occurred_at.clone();
            }
            ThreadEvent::Closed {
                occurred_at,
                reason,
                ..
            } => {
                if !super::store::closed_reason_closes_thread(reason.as_deref()) {
                    return Ok(());
                }
                thread.status = ThreadStatus::Closed;
                thread.updated_at = occurred_at.clone();
            }
            ThreadEvent::Created { .. } => {}
        }
        Ok(())
    }

    fn notify_change(&self) {
        if let Some(tx) = &self.change_tx {
            let _ = tx.send(());
        }
    }

    fn mark_activity(&self) {
        self.activity_generation.fetch_add(1, Ordering::Relaxed);
    }
}

async fn wait_for_prompt_cancellation(cancellation: &mut Option<PromptCancellation>) {
    match cancellation {
        Some(cancellation) => cancellation.cancelled().await,
        None => std::future::pending().await,
    }
}

fn runtime_stopped_error() -> acp::Error {
    acp::Error::new(-32603, "thread runtime stopped")
}

fn latest_session_for_host(thread: &WorkspaceThread) -> Option<String> {
    thread
        .agent_sessions
        .get(&thread.host_binding)
        .and_then(|sessions| sessions.last())
        .map(|session| session.session_id.clone())
}

fn host_startup_session(
    route: &RouteKey,
    runtime_session_id: Option<String>,
    thread: &WorkspaceThread,
) -> StartupSession {
    let Some(session_id) = runtime_session_id.or_else(|| latest_session_for_host(thread)) else {
        return StartupSession::Fresh;
    };
    if route_allows_startup_replay(route) {
        if thread.host_binding.agent_id == "gemini" {
            return StartupSession::ResumeOnly(session_id);
        }
        StartupSession::Load(session_id)
    } else {
        StartupSession::Resume(session_id)
    }
}

pub(crate) fn route_allows_startup_replay(route: &RouteKey) -> bool {
    channel_traits(&route.channel_kind).startup_replay
}

fn first_text(content_blocks: &[acp::ContentBlock]) -> Option<String> {
    content_blocks.iter().find_map(|block| match block {
        acp::ContentBlock::Text(text) => {
            let trimmed = text.text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.chars().take(240).collect())
            }
        }
        _ => None,
    })
}

fn aggregate_turn_status(
    agent_ids: &[ThreadAgentId],
    agents: &BTreeMap<ThreadAgentId, ThreadAgent>,
) -> ThreadAgentStatus {
    let statuses: Vec<ThreadAgentStatus> = agent_ids
        .iter()
        .filter_map(|agent_id| agents.get(agent_id).map(|agent| agent.status))
        .collect();
    if statuses.contains(&ThreadAgentStatus::Error) {
        ThreadAgentStatus::Error
    } else if statuses.contains(&ThreadAgentStatus::Running) {
        ThreadAgentStatus::Running
    } else if !statuses.is_empty()
        && statuses
            .iter()
            .all(|status| *status == ThreadAgentStatus::Completed)
    {
        ThreadAgentStatus::Completed
    } else {
        ThreadAgentStatus::Ready
    }
}

fn subagent_assignment_prompt(agent: &ThreadAgent) -> String {
    let assignment = serde_json::json!({
        "protocol": "va-agent-protocol",
        "kind": "assignment",
        "turn_id": agent.turn_id.to_string(),
        "to_agent_id": agent.id.to_string(),
        "task": agent.task.clone().unwrap_or_default(),
        "context": {
            "name": agent.name.clone(),
            "branch": agent.branch.clone(),
            "worktree": agent.worktree.clone(),
        }
    });
    subagent_assignment_prompt_from_value(agent, &assignment)
}

fn subagent_assignment_prompt_from_value(
    agent: &ThreadAgent,
    assignment: &serde_json::Value,
) -> String {
    let report_schema = subagent_report_schema(agent);
    format!(
        "You are a VibeAround subagent named {name}.\n\
         Work only inside your current git worktree. Do not merge branches or clean up worktrees.\n\
         Complete the assignment independently. You may stream progress and tool output normally.\n\
         When the assignment is complete, end your final assistant content with exactly one `va-agent-protocol` report envelope. Do not put any prose after that envelope.\n\
         The JSON inside the final report must match this report shape:\n\
         <va-agent-protocol>\n{report_schema}\n</va-agent-protocol>\n\n\
         <va-agent-protocol>\n{assignment}\n</va-agent-protocol>",
        name = agent.name.as_str(),
        report_schema =
            serde_json::to_string_pretty(&report_schema).unwrap_or_else(|_| report_schema.to_string()),
        assignment = serde_json::to_string_pretty(assignment).unwrap_or_else(|_| assignment.to_string())
    )
}

fn subagent_report_repair_prompt(agent: &ThreadAgent, error: &str) -> String {
    let report_schema = subagent_report_schema(agent);
    format!(
        "Your previous response could not be accepted as a VibeAround subagent report.\n\
         Reason: {error}\n\n\
         Do not continue task work. Emit exactly one final `va-agent-protocol` report envelope now. \
         Do not put any prose before or after the envelope.\n\
         The JSON inside the final report must match this report shape:\n\
         <va-agent-protocol>\n{report_schema}\n</va-agent-protocol>",
        error = error.trim(),
        report_schema =
            serde_json::to_string_pretty(&report_schema).unwrap_or_else(|_| report_schema.to_string()),
    )
}

fn subagent_new_session_request(agent: &ThreadAgent) -> acp::NewSessionRequest {
    acp::NewSessionRequest::new(PathBuf::from(agent.worktree.clone()))
        .meta(subagent_session_meta(agent))
}

fn subagent_session_meta(agent: &ThreadAgent) -> acp::Meta {
    let system_prompt = subagent_system_prompt(agent);
    let mut meta = serde_json::Map::new();
    meta.insert("systemPrompt".to_string(), serde_json::json!(system_prompt));
    meta.insert(
        "vibearound".to_string(),
        serde_json::json!({
            "role": "subagent",
            "system_prompt": system_prompt,
            "turn_id": agent.turn_id.to_string(),
            "subagent_id": agent.id.to_string(),
            "subagent_name": agent.name.clone(),
        }),
    );
    meta
}

fn subagent_system_prompt(agent: &ThreadAgent) -> String {
    format!(
        "You are a VibeAround subagent named {name}. Work only inside your assigned git worktree. \
         Treat host assignments wrapped in <va-agent-protocol> as control messages. \
         You may stream ordinary progress messages for the web UI, but control/report data must be wrapped in <va-agent-protocol>. \
         Your completion report envelope must be the final content you emit, with no prose after it. \
         Do not merge branches or clean up worktrees; the host reviews and merges results.",
        name = agent.name
    )
}

fn validate_subagent_assignment(
    agent: &ThreadAgent,
    agent_id: &ThreadAgentId,
    assignment: &serde_json::Value,
) -> acp::Result<()> {
    let object = assignment
        .as_object()
        .ok_or_else(|| acp::Error::new(-32602, "assignment must be a JSON object"))?;
    require_assignment_field(object, "protocol", "va-agent-protocol")?;
    require_assignment_field(object, "kind", "assignment")?;
    require_assignment_field(object, "turn_id", agent.turn_id.as_str())?;
    require_assignment_field(object, "to_agent_id", agent_id.as_str())?;
    let task = object
        .get("task")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|task| !task.is_empty())
        .ok_or_else(|| acp::Error::new(-32602, "assignment `task` must be a non-empty string"))?;
    if task.chars().count() > 24_000 {
        return Err(acp::Error::new(-32602, "assignment `task` is too large"));
    }
    Ok(())
}

fn require_assignment_field(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
    expected: &str,
) -> acp::Result<()> {
    let actual = object
        .get(field)
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            acp::Error::new(-32602, format!("assignment `{}` must be a string", field))
        })?;
    if actual == expected {
        Ok(())
    } else {
        Err(acp::Error::new(
            -32602,
            format!(
                "assignment `{}` expected `{}`, got `{}`",
                field, expected, actual
            ),
        ))
    }
}

fn subagent_report_schema(agent: &ThreadAgent) -> serde_json::Value {
    serde_json::json!({
        "protocol": "va-agent-protocol",
        "kind": "report",
        "turn_id": agent.turn_id.to_string(),
        "from_agent_id": agent.id.to_string(),
        "status": "completed",
        "summary": "One or two sentences describing the outcome.",
        "files_changed": ["relative/path.rs"],
        "tests": ["cargo test --manifest-path path/Cargo.toml"],
        "notes": ["Important caveats, blockers, or follow-up needed."]
    })
}

async fn append_thread_event(store: &ThreadEventStore, event: &ThreadEvent) -> acp::Result<()> {
    store
        .append(event)
        .await
        .with_context(|| format!("append thread event to {:?}", store.path()))
        .map_err(|error| acp::Error::new(-32603, format!("{:#}", error)))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::workspace::registry::WorkspaceId;

    fn test_thread_agent() -> ThreadAgent {
        ThreadAgent::ready(
            ThreadAgentId::from("00000000-0000-0000-0000-000000000001"),
            "mat_a",
            "John Planner",
            "codex",
            None,
            "va/subagents/mat_a/john-planner",
            "/tmp/john-planner",
            Some("plan".to_string()),
        )
    }

    fn thread_with_sessions() -> WorkspaceThread {
        thread_with_host_session("codex", Some("profile_a"), "session-old")
    }

    fn thread_with_host_session(
        agent_id: &str,
        profile_id: Option<&str>,
        session_id: &str,
    ) -> WorkspaceThread {
        let host = HostBinding::new(agent_id, profile_id.map(ToOwned::to_owned));
        let mut sessions = BTreeMap::new();
        sessions.insert(
            host.clone(),
            vec![super::super::store::AgentSessionRef {
                agent_id: agent_id.to_string(),
                profile_id: profile_id.map(ToOwned::to_owned),
                session_id: session_id.to_string(),
                observed_at: "2026-01-01T00:00:00.000Z".to_string(),
            }],
        );
        WorkspaceThread {
            id: WorkspaceThreadId::from("wt_a"),
            workspace_id: WorkspaceId::from("ws_a"),
            host_binding: host,
            status: ThreadStatus::Open,
            first_user_prompt: None,
            agent_sessions: sessions,
            agents: BTreeMap::new(),
            multi_agent_turns: BTreeMap::new(),
            created_at: "2026-01-01T00:00:00.000Z".to_string(),
            updated_at: "2026-01-01T00:00:00.000Z".to_string(),
        }
    }

    #[tokio::test]
    async fn runtime_initial_state_uses_latest_host_session() {
        let runtime = Arc::new(ThreadRuntime::new(
            thread_with_sessions(),
            PathBuf::from("/tmp/project"),
            ThreadEventStore::new("/tmp/unused.jsonl"),
        ));

        let state = runtime.state().await;

        assert_eq!(state.session_id.as_deref(), Some("session-old"));
    }

    #[tokio::test]
    async fn turn_owner_serializes_busy_state() {
        let runtime = Arc::new(ThreadRuntime::new(
            thread_with_sessions(),
            PathBuf::from("/tmp/project"),
            ThreadEventStore::new("/tmp/unused.jsonl"),
        ));
        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        runtime
            .owner_tx
            .send(ThreadOwnerCommand::Probe {
                started: started_tx,
                release: release_rx,
            })
            .unwrap();

        started_rx.await.expect("prompt scope started");
        assert!(runtime.state().await.busy);

        let mut turn_state = runtime.turn_state.clone();
        assert!(turn_state.borrow_and_update().busy);
        release_tx.send(()).unwrap();
        turn_state.changed().await.unwrap();

        assert!(!runtime.state().await.busy);
    }

    #[tokio::test]
    async fn cancel_signals_active_turn_before_agent_lookup() {
        let runtime = Arc::new(ThreadRuntime::new(
            thread_with_sessions(),
            PathBuf::from("/tmp/project"),
            ThreadEventStore::new("/tmp/unused.jsonl"),
        ));
        let _target_guard = runtime
            .active_turn_target
            .install(ChannelTarget::for_route(RouteKey::new("web", "chat-1")));
        let (_, mut cancelled) = runtime
            .active_turn_target
            .current_with_cancellation()
            .expect("active target");

        assert!(runtime.cancel().await.is_err(), "runtime has no live agent");
        cancelled
            .wait_for(|is_cancelled| *is_cancelled)
            .await
            .expect("active turn cancellation sender remains live");
    }

    #[test]
    fn web_routes_load_previous_host_session_for_playback() {
        let route = RouteKey::new("web", "chat-1");

        let startup_session = host_startup_session(&route, None, &thread_with_sessions());

        assert_eq!(
            startup_session,
            StartupSession::Load("session-old".to_string())
        );
    }

    #[test]
    fn tui_routes_load_previous_host_session_for_playback() {
        let route = RouteKey::new("tui", "chat-1");

        let startup_session = host_startup_session(&route, None, &thread_with_sessions());

        assert_eq!(
            startup_session,
            StartupSession::Load("session-old".to_string())
        );
    }

    #[test]
    fn web_gemini_routes_resume_without_load_fallback() {
        let route = RouteKey::new("web", "chat-1");
        let thread = thread_with_host_session("gemini", None, "gemini-session");

        let startup_session = host_startup_session(&route, None, &thread);

        assert_eq!(
            startup_session,
            StartupSession::ResumeOnly("gemini-session".to_string())
        );
    }

    #[test]
    fn im_routes_resume_previous_host_session_without_playback() {
        let route = RouteKey::new("slack", "dm-1");

        let startup_session = host_startup_session(
            &route,
            Some("runtime-session".to_string()),
            &thread_with_sessions(),
        );

        assert_eq!(
            startup_session,
            StartupSession::Resume("runtime-session".to_string())
        );
    }

    #[test]
    fn routes_without_known_session_start_fresh() {
        let route = RouteKey::new("slack", "dm-1");
        let thread = WorkspaceThread {
            id: WorkspaceThreadId::from("wt_a"),
            workspace_id: WorkspaceId::from("ws_a"),
            host_binding: HostBinding::new("codex", Some("direct".to_string())),
            status: ThreadStatus::Open,
            first_user_prompt: None,
            agent_sessions: BTreeMap::new(),
            agents: BTreeMap::new(),
            multi_agent_turns: BTreeMap::new(),
            created_at: "2026-01-01T00:00:00.000Z".to_string(),
            updated_at: "2026-01-01T00:00:00.000Z".to_string(),
        };

        let startup_session = host_startup_session(&route, None, &thread);

        assert_eq!(startup_session, StartupSession::Fresh);
    }

    #[tokio::test]
    async fn selecting_the_current_profile_is_a_noop() {
        let path = std::env::temp_dir().join(format!(
            "vibearound-runtime-noop-profile-{}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let runtime = Arc::new(ThreadRuntime::new(
            thread_with_sessions(),
            PathBuf::from("/tmp/project"),
            ThreadEventStore::new(&path),
        ));

        runtime
            .switch_profile_preserving_session(HostBinding::new(
                "codex",
                Some("profile_a".to_string()),
            ))
            .await
            .unwrap();

        assert_eq!(
            runtime.state().await.session_id.as_deref(),
            Some("session-old")
        );
        assert!(
            !path.exists(),
            "a no-op profile selection persisted a host change"
        );
    }

    #[test]
    fn first_text_is_trimmed_and_limited() {
        let long = format!("  {}  ", "a".repeat(300));
        let blocks = vec![acp::ContentBlock::Text(acp::TextContent::new(long))];

        let text = first_text(&blocks).unwrap();

        assert_eq!(text.len(), 240);
        assert!(text.chars().all(|c| c == 'a'));
    }

    #[test]
    fn validates_matching_follow_up_assignment() {
        let agent = test_thread_agent();
        let assignment = serde_json::json!({
            "protocol": "va-agent-protocol",
            "kind": "assignment",
            "turn_id": "mat_a",
            "to_agent_id": "00000000-0000-0000-0000-000000000001",
            "task": "Run another review pass.",
            "context": { "focus": "tests" }
        });

        validate_subagent_assignment(&agent, &agent.id, &assignment).unwrap();
    }

    #[test]
    fn rejects_follow_up_assignment_for_wrong_turn() {
        let agent = test_thread_agent();
        let assignment = serde_json::json!({
            "protocol": "va-agent-protocol",
            "kind": "assignment",
            "turn_id": "mat_other",
            "to_agent_id": "00000000-0000-0000-0000-000000000001",
            "task": "Run another review pass."
        });

        let error = validate_subagent_assignment(&agent, &agent.id, &assignment).unwrap_err();

        assert!(error.message.contains("turn_id"));
    }

    #[test]
    fn rejects_follow_up_assignment_without_task() {
        let agent = test_thread_agent();
        let assignment = serde_json::json!({
            "protocol": "va-agent-protocol",
            "kind": "assignment",
            "turn_id": "mat_a",
            "to_agent_id": "00000000-0000-0000-0000-000000000001",
            "task": " "
        });

        let error = validate_subagent_assignment(&agent, &agent.id, &assignment).unwrap_err();

        assert!(error.message.contains("task"));
    }

    #[test]
    fn repair_prompt_asks_only_for_protocol_report() {
        let agent = test_thread_agent();
        let prompt = subagent_report_repair_prompt(&agent, "missing report");

        assert!(prompt.contains("missing report"));
        assert!(prompt.contains("\"kind\": \"report\""));
        assert!(prompt.contains("\"from_agent_id\": \"00000000-0000-0000-0000-000000000001\""));
        assert!(prompt.contains("Do not continue task work"));
    }
}
