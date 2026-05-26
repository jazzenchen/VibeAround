//! Runtime owner for one workspace thread.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use agent_client_protocol::schema as acp;
use anyhow::Context;
use tokio::sync::{broadcast, mpsc, Mutex};

use crate::agent::{Agent, AgentClientHandler};
use crate::routing::RouteKey;
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

struct SubagentRuntime {
    agent: Arc<Agent>,
    session_id: String,
}

#[derive(Debug, Clone)]
pub struct SubagentCompletionResult {
    pub status: ThreadAgentStatus,
    pub last_error: Option<String>,
}

#[async_trait::async_trait]
pub trait SubagentCompletionValidator: Send + Sync + 'static {
    async fn validate_completion(&self) -> Result<SubagentCompletionResult, String>;
}

pub struct ThreadRuntime {
    thread: Mutex<WorkspaceThread>,
    workspace: PathBuf,
    agent: Mutex<Option<Arc<Agent>>>,
    subagents: Mutex<BTreeMap<ThreadAgentId, SubagentRuntime>>,
    session_id: Mutex<Option<String>>,
    initialize: Mutex<Option<acp::InitializeResponse>>,
    prompt_lock: Mutex<()>,
    busy: Mutex<bool>,
    failed: Mutex<Option<String>>,
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
        Self {
            thread: Mutex::new(thread),
            workspace,
            agent: Mutex::new(None),
            subagents: Mutex::new(BTreeMap::new()),
            session_id: Mutex::new(session_id),
            initialize: Mutex::new(None),
            prompt_lock: Mutex::new(()),
            busy: Mutex::new(false),
            failed: Mutex::new(None),
            activity_generation: AtomicU64::new(0),
            store,
            change_tx,
        }
    }

    pub async fn state(&self) -> ThreadRuntimeState {
        let thread = self.thread.lock().await;
        ThreadRuntimeState {
            thread_id: thread.id.clone(),
            workspace_id: thread.workspace_id.clone(),
            host_binding: thread.host_binding.clone(),
            session_id: self.session_id.lock().await.clone(),
            workspace: self.workspace.clone(),
            busy: *self.busy.lock().await,
            failed: self.failed.lock().await.clone(),
            initialize: self.initialize.lock().await.clone(),
            agents: thread.agents.values().cloned().collect(),
            multi_agent_turns: thread.multi_agent_turns.values().cloned().collect(),
        }
    }

    /// Start the host agent and ensure a session exists, without sending a
    /// user prompt. This backs `/new` and route attachment warmup.
    pub async fn start(
        &self,
        route: &RouteKey,
        handler: Arc<dyn AgentClientHandler>,
    ) -> acp::Result<String> {
        self.mark_activity();
        let agent = self.ensure_agent(route, handler).await?;
        self.ensure_session(&agent).await
    }

    pub async fn prompt(
        &self,
        route: &RouteKey,
        content_blocks: Vec<acp::ContentBlock>,
        handler: Arc<dyn AgentClientHandler>,
    ) -> acp::Result<acp::PromptResponse> {
        let _prompt_guard = self.prompt_lock.lock().await;
        self.mark_activity();
        *self.busy.lock().await = true;
        *self.failed.lock().await = None;
        self.notify_change();

        let result = async {
            self.maybe_record_first_prompt(&content_blocks).await?;
            let agent = self.ensure_agent(route, handler).await?;
            let session_id = self.ensure_session(&agent).await?;
            agent
                .prompt(acp::PromptRequest::new(session_id, content_blocks))
                .await
        }
        .await;

        *self.busy.lock().await = false;
        if let Err(error) = &result {
            *self.failed.lock().await = Some(error.message.to_string());
        }
        self.notify_change();
        result
    }

    pub async fn cancel(&self) -> acp::Result<()> {
        self.mark_activity();
        let agent = self
            .agent
            .lock()
            .await
            .clone()
            .ok_or_else(acp::Error::method_not_found)?;
        let session_id = self
            .session_id
            .lock()
            .await
            .clone()
            .ok_or_else(acp::Error::method_not_found)?;
        agent.cancel(acp::CancelNotification::new(session_id)).await
    }

    pub async fn close(&self, reason: Option<String>) -> acp::Result<()> {
        self.mark_activity();
        if let Some(session_id) = self.session_id.lock().await.clone() {
            crate::previews::kill_by_session(&session_id);
        }
        if let Some(agent) = self.agent.lock().await.take() {
            agent.shutdown().await;
        }
        for (_, subagent) in std::mem::take(&mut *self.subagents.lock().await) {
            crate::previews::kill_by_session(&subagent.session_id);
            subagent.agent.shutdown().await;
        }
        let thread_id = self.thread.lock().await.id.clone();
        let event = ThreadEvent::closed(thread_id, reason);
        append_thread_event(&self.store, &event).await?;
        self.apply_thread_event(&event).await?;
        *self.session_id.lock().await = None;
        *self.initialize.lock().await = None;
        self.notify_change();
        Ok(())
    }

    pub async fn shutdown_host(&self) {
        self.mark_activity();
        if let Some(session_id) = self.session_id.lock().await.clone() {
            crate::previews::kill_by_session(&session_id);
        }
        if let Some(agent) = self.agent.lock().await.take() {
            agent.shutdown().await;
        }
        for (_, subagent) in std::mem::take(&mut *self.subagents.lock().await) {
            crate::previews::kill_by_session(&subagent.session_id);
            subagent.agent.shutdown().await;
        }
        *self.initialize.lock().await = None;
        *self.busy.lock().await = false;
        *self.failed.lock().await = None;
        self.notify_change();
    }

    pub fn idle_generation(&self) -> u64 {
        self.activity_generation.load(Ordering::Relaxed)
    }

    pub async fn shutdown_host_if_idle(&self, generation: u64) -> bool {
        if self.idle_generation() != generation {
            return false;
        }
        if *self.busy.lock().await {
            return false;
        }
        if !self.subagents.lock().await.is_empty() {
            return false;
        }
        if self.idle_generation() != generation {
            return false;
        }
        let has_host = self.agent.lock().await.is_some() || self.initialize.lock().await.is_some();
        if !has_host {
            return false;
        }
        if self.idle_generation() != generation {
            return false;
        }
        self.shutdown_host().await;
        true
    }

    pub async fn switch_host(
        &self,
        host_binding: HostBinding,
        context_transfer: bool,
    ) -> acp::Result<()> {
        self.mark_activity();
        if let Some(agent) = self.agent.lock().await.take() {
            agent.shutdown().await;
        }
        *self.session_id.lock().await = None;
        *self.initialize.lock().await = None;
        *self.failed.lock().await = None;

        let thread_id = self.thread.lock().await.id.clone();
        let event = ThreadEvent::host_changed(thread_id, host_binding, context_transfer);
        append_thread_event(&self.store, &event).await?;
        self.apply_thread_event(&event).await?;
        let thread = self.thread.lock().await;
        let next_session_id = latest_session_for_host(&thread);
        drop(thread);
        *self.session_id.lock().await = next_session_id;
        self.notify_change();
        Ok(())
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

    pub async fn start_subagent_assignment(
        self: &Arc<Self>,
        route: &RouteKey,
        thread_agent: ThreadAgent,
        handler: Arc<dyn AgentClientHandler>,
        status_tx: mpsc::UnboundedSender<ThreadAgent>,
        completion_validator: Option<Arc<dyn SubagentCompletionValidator>>,
    ) -> acp::Result<()> {
        self.mark_activity();
        if self.thread.lock().await.status == ThreadStatus::Closed {
            return Err(acp::Error::new(-32603, "workspace thread is closed"));
        }

        let agent = match self.spawn_subagent(route, &thread_agent, handler).await {
            Ok(agent) => agent,
            Err(error) => {
                if let Ok(Some(updated)) = self
                    .set_thread_agent_status(
                        &thread_agent.id,
                        ThreadAgentStatus::Error,
                        Some(error.message.to_string()),
                    )
                    .await
                {
                    let _ = status_tx.send(updated);
                }
                return Err(error);
            }
        };
        let session = match agent
            .new_session(acp::NewSessionRequest::new(PathBuf::from(
                thread_agent.worktree.clone(),
            )))
            .await
        {
            Ok(session) => session,
            Err(error) => {
                if let Ok(Some(updated)) = self
                    .set_thread_agent_status(
                        &thread_agent.id,
                        ThreadAgentStatus::Error,
                        Some(error.message.to_string()),
                    )
                    .await
                {
                    let _ = status_tx.send(updated);
                }
                agent.shutdown().await;
                return Err(error);
            }
        };
        let session_id = session.session_id.to_string();
        self.subagents.lock().await.insert(
            thread_agent.id.clone(),
            SubagentRuntime {
                agent: Arc::clone(&agent),
                session_id: session_id.clone(),
            },
        );

        if let Some(updated) = self
            .set_thread_agent_status(&thread_agent.id, ThreadAgentStatus::Running, None)
            .await?
        {
            let _ = status_tx.send(updated);
        }

        let prompt = subagent_assignment_prompt(&thread_agent);
        let runtime = Arc::clone(self);
        let agent_id = thread_agent.id.clone();
        tokio::spawn(async move {
            let result = agent
                .prompt(acp::PromptRequest::new(
                    session_id,
                    vec![acp::ContentBlock::Text(acp::TextContent::new(prompt))],
                ))
                .await;
            let next = match result {
                Ok(_) => match completion_validator {
                    Some(validator) => match validator.validate_completion().await {
                        Ok(completion) => {
                            runtime
                                .set_thread_agent_status(
                                    &agent_id,
                                    completion.status,
                                    completion.last_error,
                                )
                                .await
                        }
                        Err(message) => {
                            runtime
                                .set_thread_agent_status(
                                    &agent_id,
                                    ThreadAgentStatus::Error,
                                    Some(message),
                                )
                                .await
                        }
                    },
                    None => {
                        runtime
                            .set_thread_agent_status(&agent_id, ThreadAgentStatus::Completed, None)
                            .await
                    }
                },
                Err(error) => {
                    let message = error.message.to_string();
                    runtime
                        .set_thread_agent_status(
                            &agent_id,
                            ThreadAgentStatus::Error,
                            Some(message),
                        )
                        .await
                }
            };
            match next {
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
        });

        Ok(())
    }

    async fn ensure_agent(
        &self,
        route: &RouteKey,
        handler: Arc<dyn AgentClientHandler>,
    ) -> acp::Result<Arc<Agent>> {
        if let Some(agent) = self.agent.lock().await.clone() {
            return Ok(agent);
        }

        let thread = self.thread.lock().await.clone();
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
        let resume_session_id = self
            .session_id
            .lock()
            .await
            .clone()
            .or_else(|| latest_session_for_host(&thread));

        std::fs::create_dir_all(&self.workspace).map_err(|error| {
            acp::Error::new(
                -32603,
                format!("failed to create workspace {:?}: {}", self.workspace, error),
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
                &self.workspace,
                route,
            )
            .map_err(|error| acp::Error::new(-32603, format!("{:#}", error)))?;
            env_vars.extend(applied.env);
            extra_args.extend(applied.command_args);
        }

        let ready = Agent::spawn(
            agent_id.clone(),
            route,
            &self.workspace,
            resume_session_id.clone(),
            handler,
            extra_args,
            env_vars,
        )
        .await
        .map_err(|error| {
            let message = format!("{:#}", error);
            acp::Error::new(-32603, message)
        })?;

        *self.initialize.lock().await = Some(ready.initialize.clone());
        *self.agent.lock().await = Some(Arc::clone(&ready.agent));
        *self.failed.lock().await = None;
        self.notify_change();

        if let Some(session_id) = ready.startup_session_id {
            self.observe_session(&agent_id, thread.host_binding.profile_id, &session_id)
                .await?;
        } else if resume_session_id.is_some() {
            // The bridge falls back to a fresh agent when load_session fails.
            // Clear the stale candidate so ensure_session creates a real one.
            *self.session_id.lock().await = None;
        }

        Ok(ready.agent)
    }

    async fn spawn_subagent(
        &self,
        route: &RouteKey,
        thread_agent: &ThreadAgent,
        handler: Arc<dyn AgentClientHandler>,
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
                format!("failed to create subagent worktree {:?}: {}", worktree, error),
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

        let ready = Agent::spawn(
            agent_id,
            route,
            &worktree,
            None,
            handler,
            extra_args,
            env_vars,
        )
        .await
        .map_err(|error| acp::Error::new(-32603, format!("{:#}", error)))?;
        Ok(ready.agent)
    }

    async fn ensure_session(&self, agent: &Arc<Agent>) -> acp::Result<String> {
        if let Some(session_id) = self.session_id.lock().await.clone() {
            return Ok(session_id);
        }

        let response = agent
            .new_session(acp::NewSessionRequest::new(self.workspace.clone()))
            .await?;
        let session_id = response.session_id.to_string();
        let host = self.thread.lock().await.host_binding.clone();
        self.observe_session(&host.agent_id, host.profile_id, &session_id)
            .await?;
        Ok(session_id)
    }

    async fn observe_session(
        &self,
        agent_id: &str,
        profile_id: Option<String>,
        session_id: &str,
    ) -> acp::Result<()> {
        if self.session_id.lock().await.as_deref() == Some(session_id) {
            return Ok(());
        }

        let binding = HostBinding::new(agent_id.to_string(), profile_id.clone());
        {
            let thread = self.thread.lock().await;
            if thread.has_agent_session(&binding, session_id) {
                *self.session_id.lock().await = Some(session_id.to_string());
                return Ok(());
            }
        }

        let thread_id = self.thread.lock().await.id.clone();
        let event = ThreadEvent::agent_session_observed(
            thread_id,
            agent_id.to_string(),
            profile_id,
            session_id.to_string(),
        );
        append_thread_event(&self.store, &event).await?;
        self.apply_thread_event(&event).await?;
        *self.session_id.lock().await = Some(session_id.to_string());
        self.notify_change();
        Ok(())
    }

    async fn set_thread_agent_status(
        &self,
        agent_id: &ThreadAgentId,
        status: ThreadAgentStatus,
        last_error: Option<String>,
    ) -> acp::Result<Option<ThreadAgent>> {
        let thread_id = self.thread.lock().await.id.clone();
        let event = ThreadEvent::thread_agent_status_changed(
            thread_id,
            agent_id.clone(),
            status,
            last_error,
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
                last_error,
                ..
            } => {
                let turn_id = if let Some(agent) = thread.agents.get_mut(agent_id) {
                    agent.status = *status;
                    agent.last_error = last_error.clone();
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

fn latest_session_for_host(thread: &WorkspaceThread) -> Option<String> {
    thread
        .agent_sessions
        .get(&thread.host_binding)
        .and_then(|sessions| sessions.last())
        .map(|session| session.session_id.clone())
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
    if statuses.iter().any(|status| *status == ThreadAgentStatus::Error) {
        ThreadAgentStatus::Error
    } else if statuses
        .iter()
        .any(|status| *status == ThreadAgentStatus::Running)
    {
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
    let report_schema = serde_json::json!({
        "protocol": "va-agent-protocol",
        "kind": "report",
        "turn_id": agent.turn_id.to_string(),
        "from_agent_id": agent.id.to_string(),
        "status": "completed",
        "summary": "One or two sentences describing the outcome.",
        "files_changed": ["relative/path.rs"],
        "tests": ["cargo test --manifest-path path/Cargo.toml"],
        "notes": ["Important caveats, blockers, or follow-up needed."]
    });
    format!(
        "You are a VibeAround subagent named {name}.\n\
         Work only inside your current git worktree. Do not merge branches or clean up worktrees.\n\
         Complete the assignment independently, then report back to the host using only a `va-agent-protocol` report envelope.\n\
         The final assistant message must contain exactly one XML envelope and no other prose. The JSON inside must match this report shape:\n\
         <va-agent-protocol>\n{report_schema}\n</va-agent-protocol>\n\n\
         <va-agent-protocol>\n{assignment}\n</va-agent-protocol>",
        name = agent.name.as_str(),
        report_schema = serde_json::to_string_pretty(&report_schema)
            .unwrap_or_else(|_| report_schema.to_string()),
        assignment = serde_json::to_string_pretty(&assignment)
            .unwrap_or_else(|_| assignment.to_string())
    )
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

    fn thread_with_sessions() -> WorkspaceThread {
        let host = HostBinding::new("codex", Some("profile_a".to_string()));
        let mut sessions = BTreeMap::new();
        sessions.insert(
            host.clone(),
            vec![super::super::store::AgentSessionRef {
                agent_id: "codex".to_string(),
                profile_id: Some("profile_a".to_string()),
                session_id: "session-old".to_string(),
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

    #[test]
    fn runtime_initial_state_uses_latest_host_session() {
        let runtime = ThreadRuntime::new(
            thread_with_sessions(),
            PathBuf::from("/tmp/project"),
            ThreadEventStore::new("/tmp/unused.jsonl"),
        );

        let state = futures::executor::block_on(runtime.state());

        assert_eq!(state.session_id.as_deref(), Some("session-old"));
    }

    #[test]
    fn first_text_is_trimmed_and_limited() {
        let long = format!("  {}  ", "a".repeat(300));
        let blocks = vec![acp::ContentBlock::Text(acp::TextContent::new(long))];

        let text = first_text(&blocks).unwrap();

        assert_eq!(text.len(), 240);
        assert!(text.chars().all(|c| c == 'a'));
    }
}
