use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::storage::jsonl;

use super::super::registry::WorkspaceId;
use super::super::store::{event_id, now};

const SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkspaceThreadId(String);

impl WorkspaceThreadId {
    pub fn new() -> Self {
        Self(format!("wt_{}", Uuid::new_v4().simple()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for WorkspaceThreadId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<String> for WorkspaceThreadId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for WorkspaceThreadId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl std::fmt::Display for WorkspaceThreadId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MultiAgentTurnId(String);

impl MultiAgentTurnId {
    pub fn new() -> Self {
        Self(format!("mat_{}", Uuid::new_v4().simple()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for MultiAgentTurnId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<String> for MultiAgentTurnId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for MultiAgentTurnId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl std::fmt::Display for MultiAgentTurnId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ThreadAgentId(String);

impl ThreadAgentId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ThreadAgentId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<String> for ThreadAgentId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for ThreadAgentId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl std::fmt::Display for ThreadAgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct HostBinding {
    pub agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
}

impl HostBinding {
    pub fn new(agent_id: impl Into<String>, profile_id: Option<String>) -> Self {
        Self {
            agent_id: agent_id.into(),
            profile_id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadStatus {
    Open,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MultiAgentTurnMode {
    Parallel,
    Collaboration,
    Brainstorming,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadAgentStatus {
    Ready,
    Running,
    Completed,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultiAgentTurn {
    pub id: MultiAgentTurnId,
    pub mode: MultiAgentTurnMode,
    pub status: ThreadAgentStatus,
    #[serde(rename = "agents")]
    pub agent_ids: Vec<ThreadAgentId>,
    pub created_at: String,
    pub updated_at: String,
}

impl MultiAgentTurn {
    pub fn new(
        id: impl Into<MultiAgentTurnId>,
        mode: MultiAgentTurnMode,
        agent_ids: Vec<ThreadAgentId>,
    ) -> Self {
        let timestamp = now();
        Self {
            id: id.into(),
            mode,
            status: ThreadAgentStatus::Ready,
            agent_ids,
            created_at: timestamp.clone(),
            updated_at: timestamp,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadAgent {
    pub id: ThreadAgentId,
    pub turn_id: MultiAgentTurnId,
    pub name: String,
    pub agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub status: ThreadAgentStatus,
    pub branch: String,
    pub worktree: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
}

impl ThreadAgent {
    pub fn ready(
        id: impl Into<ThreadAgentId>,
        turn_id: impl Into<MultiAgentTurnId>,
        name: impl Into<String>,
        agent_id: impl Into<String>,
        profile_id: Option<String>,
        branch: impl Into<String>,
        worktree: impl Into<String>,
        task: Option<String>,
    ) -> Self {
        let timestamp = now();
        Self {
            id: id.into(),
            turn_id: turn_id.into(),
            name: name.into(),
            agent_id: agent_id.into(),
            profile_id,
            session_id: None,
            status: ThreadAgentStatus::Ready,
            branch: branch.into(),
            worktree: worktree.into(),
            task,
            last_error: None,
            report: None,
            created_at: timestamp.clone(),
            updated_at: timestamp,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSessionRef {
    pub agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    pub session_id: String,
    pub observed_at: String,
}

impl AgentSessionRef {
    pub fn binding(&self) -> HostBinding {
        HostBinding {
            agent_id: self.agent_id.clone(),
            profile_id: self.profile_id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceThread {
    pub id: WorkspaceThreadId,
    pub workspace_id: WorkspaceId,
    pub host_binding: HostBinding,
    pub status: ThreadStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_user_prompt: Option<String>,
    pub agent_sessions: BTreeMap<HostBinding, Vec<AgentSessionRef>>,
    #[serde(default)]
    pub agents: BTreeMap<ThreadAgentId, ThreadAgent>,
    #[serde(default)]
    pub multi_agent_turns: BTreeMap<MultiAgentTurnId, MultiAgentTurn>,
    pub created_at: String,
    pub updated_at: String,
}

impl WorkspaceThread {
    pub fn has_agent_session(&self, binding: &HostBinding, session_id: &str) -> bool {
        self.agent_sessions.get(binding).is_some_and(|sessions| {
            sessions
                .iter()
                .any(|session| session.session_id == session_id)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ThreadEvent {
    Created {
        schema_version: u8,
        event_id: String,
        occurred_at: String,
        thread_id: WorkspaceThreadId,
        workspace_id: WorkspaceId,
        host_binding: HostBinding,
    },
    FirstUserPromptSet {
        schema_version: u8,
        event_id: String,
        occurred_at: String,
        thread_id: WorkspaceThreadId,
        prompt: String,
    },
    HostChanged {
        schema_version: u8,
        event_id: String,
        occurred_at: String,
        thread_id: WorkspaceThreadId,
        host_binding: HostBinding,
        context_transfer: bool,
    },
    AgentSessionObserved {
        schema_version: u8,
        event_id: String,
        occurred_at: String,
        thread_id: WorkspaceThreadId,
        agent_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        profile_id: Option<String>,
        session_id: String,
    },
    MultiAgentTurnInitialized {
        schema_version: u8,
        event_id: String,
        occurred_at: String,
        thread_id: WorkspaceThreadId,
        turn: MultiAgentTurn,
        agents: Vec<ThreadAgent>,
    },
    ThreadAgentStatusChanged {
        schema_version: u8,
        event_id: String,
        occurred_at: String,
        thread_id: WorkspaceThreadId,
        agent_id: ThreadAgentId,
        status: ThreadAgentStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        last_error: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        report: Option<serde_json::Value>,
    },
    Closed {
        schema_version: u8,
        event_id: String,
        occurred_at: String,
        thread_id: WorkspaceThreadId,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

impl ThreadEvent {
    pub fn created(
        thread_id: impl Into<WorkspaceThreadId>,
        workspace_id: impl Into<WorkspaceId>,
        host_binding: HostBinding,
    ) -> Self {
        Self::Created {
            schema_version: SCHEMA_VERSION,
            event_id: event_id(),
            occurred_at: now(),
            thread_id: thread_id.into(),
            workspace_id: workspace_id.into(),
            host_binding,
        }
    }

    pub fn first_user_prompt_set(
        thread_id: impl Into<WorkspaceThreadId>,
        prompt: impl Into<String>,
    ) -> Self {
        Self::FirstUserPromptSet {
            schema_version: SCHEMA_VERSION,
            event_id: event_id(),
            occurred_at: now(),
            thread_id: thread_id.into(),
            prompt: prompt.into(),
        }
    }

    pub fn host_changed(
        thread_id: impl Into<WorkspaceThreadId>,
        host_binding: HostBinding,
        context_transfer: bool,
    ) -> Self {
        Self::HostChanged {
            schema_version: SCHEMA_VERSION,
            event_id: event_id(),
            occurred_at: now(),
            thread_id: thread_id.into(),
            host_binding,
            context_transfer,
        }
    }

    pub fn agent_session_observed(
        thread_id: impl Into<WorkspaceThreadId>,
        agent_id: impl Into<String>,
        profile_id: Option<String>,
        session_id: impl Into<String>,
    ) -> Self {
        Self::AgentSessionObserved {
            schema_version: SCHEMA_VERSION,
            event_id: event_id(),
            occurred_at: now(),
            thread_id: thread_id.into(),
            agent_id: agent_id.into(),
            profile_id,
            session_id: session_id.into(),
        }
    }

    pub fn multi_agent_turn_initialized(
        thread_id: impl Into<WorkspaceThreadId>,
        turn: MultiAgentTurn,
        agents: Vec<ThreadAgent>,
    ) -> Self {
        Self::MultiAgentTurnInitialized {
            schema_version: SCHEMA_VERSION,
            event_id: event_id(),
            occurred_at: now(),
            thread_id: thread_id.into(),
            turn,
            agents,
        }
    }

    pub fn thread_agent_status_changed(
        thread_id: impl Into<WorkspaceThreadId>,
        agent_id: impl Into<ThreadAgentId>,
        status: ThreadAgentStatus,
        last_error: Option<String>,
        report: Option<serde_json::Value>,
    ) -> Self {
        Self::thread_agent_status_changed_with_session(
            thread_id, agent_id, status, None, last_error, report,
        )
    }

    pub fn thread_agent_status_changed_with_session(
        thread_id: impl Into<WorkspaceThreadId>,
        agent_id: impl Into<ThreadAgentId>,
        status: ThreadAgentStatus,
        session_id: Option<String>,
        last_error: Option<String>,
        report: Option<serde_json::Value>,
    ) -> Self {
        Self::ThreadAgentStatusChanged {
            schema_version: SCHEMA_VERSION,
            event_id: event_id(),
            occurred_at: now(),
            thread_id: thread_id.into(),
            agent_id: agent_id.into(),
            status,
            session_id,
            last_error,
            report,
        }
    }

    pub fn closed(thread_id: impl Into<WorkspaceThreadId>, reason: Option<String>) -> Self {
        Self::Closed {
            schema_version: SCHEMA_VERSION,
            event_id: event_id(),
            occurred_at: now(),
            thread_id: thread_id.into(),
            reason,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThreadProjection {
    threads: BTreeMap<WorkspaceThreadId, WorkspaceThread>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ThreadProjectionError {
    #[error("thread {thread_id} is already registered")]
    DuplicateThread { thread_id: WorkspaceThreadId },
    #[error("thread {thread_id} does not exist")]
    UnknownThread { thread_id: WorkspaceThreadId },
    #[error("thread agent {agent_id} does not exist")]
    UnknownThreadAgent { agent_id: ThreadAgentId },
}

impl ThreadProjection {
    pub fn from_events(events: &[ThreadEvent]) -> Result<Self, ThreadProjectionError> {
        let mut projection = Self::default();
        for event in events {
            projection.apply(event)?;
        }
        Ok(projection)
    }

    pub fn apply(&mut self, event: &ThreadEvent) -> Result<(), ThreadProjectionError> {
        match event {
            ThreadEvent::Created {
                occurred_at,
                thread_id,
                workspace_id,
                host_binding,
                ..
            } => self.create(
                thread_id.clone(),
                workspace_id.clone(),
                host_binding.clone(),
                occurred_at.clone(),
            ),
            ThreadEvent::FirstUserPromptSet {
                occurred_at,
                thread_id,
                prompt,
                ..
            } => {
                let thread = self.thread_mut(thread_id)?;
                if thread.first_user_prompt.is_none() {
                    thread.first_user_prompt = Some(prompt.clone());
                }
                thread.updated_at = occurred_at.clone();
                Ok(())
            }
            ThreadEvent::HostChanged {
                occurred_at,
                thread_id,
                host_binding,
                ..
            } => {
                let thread = self.thread_mut(thread_id)?;
                thread.host_binding = host_binding.clone();
                thread.updated_at = occurred_at.clone();
                Ok(())
            }
            ThreadEvent::AgentSessionObserved {
                occurred_at,
                thread_id,
                agent_id,
                profile_id,
                session_id,
                ..
            } => {
                let thread = self.thread_mut(thread_id)?;
                let session = AgentSessionRef {
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
                Ok(())
            }
            ThreadEvent::MultiAgentTurnInitialized {
                occurred_at,
                thread_id,
                turn,
                agents,
                ..
            } => {
                let thread = self.thread_mut(thread_id)?;
                thread
                    .multi_agent_turns
                    .insert(turn.id.clone(), turn.clone());
                for agent in agents {
                    thread.agents.insert(agent.id.clone(), agent.clone());
                }
                thread.updated_at = occurred_at.clone();
                Ok(())
            }
            ThreadEvent::ThreadAgentStatusChanged {
                occurred_at,
                thread_id,
                agent_id,
                status,
                session_id,
                last_error,
                report,
                ..
            } => {
                let thread = self.thread_mut(thread_id)?;
                let turn_id = {
                    let agent = thread.agents.get_mut(agent_id).ok_or_else(|| {
                        ThreadProjectionError::UnknownThreadAgent {
                            agent_id: agent_id.clone(),
                        }
                    })?;
                    agent.status = *status;
                    if session_id.is_some() {
                        agent.session_id = session_id.clone();
                    }
                    agent.last_error = last_error.clone();
                    agent.report = report.clone();
                    agent.updated_at = occurred_at.clone();
                    agent.turn_id.clone()
                };
                if let Some(agent_ids) = thread
                    .multi_agent_turns
                    .get(&turn_id)
                    .map(|turn| turn.agent_ids.clone())
                {
                    let status = aggregate_turn_status(&agent_ids, &thread.agents);
                    let turn = thread
                        .multi_agent_turns
                        .get_mut(&turn_id)
                        .expect("turn existed when aggregating status");
                    turn.status = status;
                    turn.updated_at = occurred_at.clone();
                }
                thread.updated_at = occurred_at.clone();
                Ok(())
            }
            ThreadEvent::Closed {
                occurred_at,
                thread_id,
                reason,
                ..
            } => {
                if !closed_reason_closes_thread(reason.as_deref()) {
                    return Ok(());
                }
                let thread = self.thread_mut(thread_id)?;
                thread.status = ThreadStatus::Closed;
                thread.updated_at = occurred_at.clone();
                Ok(())
            }
        }
    }

    pub fn get(&self, id: &WorkspaceThreadId) -> Option<&WorkspaceThread> {
        self.threads.get(id)
    }

    pub fn all(&self) -> impl Iterator<Item = &WorkspaceThread> {
        self.threads.values()
    }

    pub fn for_workspace<'a>(
        &'a self,
        workspace_id: &'a WorkspaceId,
        include_closed: bool,
    ) -> impl Iterator<Item = &'a WorkspaceThread> + 'a {
        self.threads.values().filter(move |thread| {
            &thread.workspace_id == workspace_id
                && (include_closed || thread.status == ThreadStatus::Open)
        })
    }

    fn create(
        &mut self,
        thread_id: WorkspaceThreadId,
        workspace_id: WorkspaceId,
        host_binding: HostBinding,
        occurred_at: String,
    ) -> Result<(), ThreadProjectionError> {
        if self.threads.contains_key(&thread_id) {
            return Err(ThreadProjectionError::DuplicateThread { thread_id });
        }

        self.threads.insert(
            thread_id.clone(),
            WorkspaceThread {
                id: thread_id,
                workspace_id,
                host_binding,
                status: ThreadStatus::Open,
                first_user_prompt: None,
                agent_sessions: BTreeMap::new(),
                agents: BTreeMap::new(),
                multi_agent_turns: BTreeMap::new(),
                created_at: occurred_at.clone(),
                updated_at: occurred_at,
            },
        );
        Ok(())
    }

    fn thread_mut(
        &mut self,
        thread_id: &WorkspaceThreadId,
    ) -> Result<&mut WorkspaceThread, ThreadProjectionError> {
        self.threads
            .get_mut(thread_id)
            .ok_or_else(|| ThreadProjectionError::UnknownThread {
                thread_id: thread_id.clone(),
            })
    }
}

pub(crate) fn closed_reason_closes_thread(reason: Option<&str>) -> bool {
    !matches!(
        reason,
        None | Some("web idle timeout") | Some("web resume aborted")
    )
}

fn aggregate_turn_status(
    agent_ids: &[ThreadAgentId],
    agents: &BTreeMap<ThreadAgentId, ThreadAgent>,
) -> ThreadAgentStatus {
    let statuses: Vec<ThreadAgentStatus> = agent_ids
        .iter()
        .filter_map(|agent_id| agents.get(agent_id).map(|agent| agent.status))
        .collect();
    if statuses
        .iter()
        .any(|status| *status == ThreadAgentStatus::Error)
    {
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

#[derive(Debug, Clone)]
pub struct ThreadEventStore {
    path: PathBuf,
}

impl ThreadEventStore {
    pub fn default_path() -> PathBuf {
        crate::config::migrate_legacy_state_file("workspace-threads.jsonl")
    }

    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn append(&self, event: &ThreadEvent) -> jsonl::Result<()> {
        jsonl::append(&self.path, event).await
    }

    pub async fn read_events(&self) -> jsonl::Result<Vec<ThreadEvent>> {
        jsonl::read_all(&self.path).await
    }

    pub async fn load_projection(&self) -> Result<ThreadProjection, ThreadStoreLoadError> {
        let events = self.read_events().await?;
        Ok(ThreadProjection::from_events(&events)?)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ThreadStoreLoadError {
    #[error(transparent)]
    Jsonl(#[from] jsonl::JsonlError),
    #[error(transparent)]
    Projection(#[from] ThreadProjectionError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_tracks_thread_lifecycle() {
        let thread_id = WorkspaceThreadId::from("wt_a");
        let workspace_id = WorkspaceId::from("ws_a");
        let codex = HostBinding::new("codex", Some("profile_a".to_string()));
        let claude = HostBinding::new("claude", None);
        let events = vec![
            ThreadEvent::created(thread_id.clone(), workspace_id.clone(), codex.clone()),
            ThreadEvent::first_user_prompt_set(thread_id.clone(), "build this"),
            ThreadEvent::agent_session_observed(
                thread_id.clone(),
                "codex",
                Some("profile_a".to_string()),
                "session-1",
            ),
            ThreadEvent::host_changed(thread_id.clone(), claude.clone(), true),
            ThreadEvent::multi_agent_turn_initialized(
                thread_id.clone(),
                MultiAgentTurn::new(
                    "mat_a",
                    MultiAgentTurnMode::Parallel,
                    vec![ThreadAgentId::from("00000000-0000-0000-0000-000000000001")],
                ),
                vec![ThreadAgent::ready(
                    "00000000-0000-0000-0000-000000000001",
                    "mat_a",
                    "John Planner",
                    "codex",
                    None,
                    "va/subagents/mat_a/john-planner",
                    "/tmp/john-planner",
                    Some("plan".to_string()),
                )],
            ),
            ThreadEvent::closed(thread_id.clone(), Some("done".to_string())),
        ];

        let projection = ThreadProjection::from_events(&events).unwrap();
        let thread = projection.get(&thread_id).unwrap();

        assert_eq!(thread.workspace_id, workspace_id);
        assert_eq!(thread.host_binding, claude);
        assert_eq!(thread.status, ThreadStatus::Closed);
        assert_eq!(thread.first_user_prompt.as_deref(), Some("build this"));
        assert_eq!(
            thread.agent_sessions.get(&codex).unwrap()[0].session_id,
            "session-1"
        );
        assert_eq!(thread.multi_agent_turns.len(), 1);
        assert_eq!(thread.agents.len(), 1);
    }

    #[test]
    fn first_prompt_is_not_overwritten() {
        let thread_id = WorkspaceThreadId::from("wt_a");
        let events = vec![
            ThreadEvent::created(
                thread_id.clone(),
                WorkspaceId::from("ws_a"),
                HostBinding::new("codex", None),
            ),
            ThreadEvent::first_user_prompt_set(thread_id.clone(), "first"),
            ThreadEvent::first_user_prompt_set(thread_id.clone(), "second"),
        ];

        let projection = ThreadProjection::from_events(&events).unwrap();

        assert_eq!(
            projection
                .get(&thread_id)
                .unwrap()
                .first_user_prompt
                .as_deref(),
            Some("first")
        );
    }

    #[test]
    fn web_lifecycle_close_events_do_not_close_threads() {
        let thread_id = WorkspaceThreadId::from("wt_a");
        for reason in [None, Some("web idle timeout"), Some("web resume aborted")] {
            let events = vec![
                ThreadEvent::created(
                    thread_id.clone(),
                    WorkspaceId::from("ws_a"),
                    HostBinding::new("codex", Some("direct".to_string())),
                ),
                ThreadEvent::closed(thread_id.clone(), reason.map(str::to_string)),
            ];

            let projection = ThreadProjection::from_events(&events).unwrap();

            assert_eq!(
                projection.get(&thread_id).unwrap().status,
                ThreadStatus::Open
            );
        }
    }

    #[test]
    fn agent_status_updates_aggregate_turn_status() {
        let thread_id = WorkspaceThreadId::from("wt_a");
        let turn_id = MultiAgentTurnId::from("mat_a");
        let first_id = ThreadAgentId::from("00000000-0000-0000-0000-000000000001");
        let second_id = ThreadAgentId::from("00000000-0000-0000-0000-000000000002");
        let events = vec![
            ThreadEvent::created(
                thread_id.clone(),
                WorkspaceId::from("ws_a"),
                HostBinding::new("codex", None),
            ),
            ThreadEvent::multi_agent_turn_initialized(
                thread_id.clone(),
                MultiAgentTurn::new(
                    turn_id.clone(),
                    MultiAgentTurnMode::Parallel,
                    vec![first_id.clone(), second_id.clone()],
                ),
                vec![
                    ThreadAgent::ready(
                        first_id.clone(),
                        turn_id.clone(),
                        "John Planner",
                        "codex",
                        None,
                        "va/subagents/mat_a/john-planner",
                        "/tmp/john-planner",
                        Some("plan".to_string()),
                    ),
                    ThreadAgent::ready(
                        second_id.clone(),
                        turn_id.clone(),
                        "Jane Builder",
                        "codex",
                        None,
                        "va/subagents/mat_a/jane-builder",
                        "/tmp/jane-builder",
                        Some("build".to_string()),
                    ),
                ],
            ),
            ThreadEvent::thread_agent_status_changed_with_session(
                thread_id.clone(),
                first_id.clone(),
                ThreadAgentStatus::Running,
                Some("subagent-session-1".to_string()),
                None,
                None,
            ),
            ThreadEvent::thread_agent_status_changed(
                thread_id.clone(),
                first_id.clone(),
                ThreadAgentStatus::Completed,
                None,
                None,
            ),
            ThreadEvent::thread_agent_status_changed(
                thread_id,
                second_id,
                ThreadAgentStatus::Completed,
                None,
                None,
            ),
        ];

        let projection = ThreadProjection::from_events(&events).unwrap();
        let thread = projection.get(&WorkspaceThreadId::from("wt_a")).unwrap();

        assert_eq!(
            thread.multi_agent_turns.get(&turn_id).unwrap().status,
            ThreadAgentStatus::Completed
        );
        assert_eq!(
            thread.agents.get(&first_id).unwrap().session_id.as_deref(),
            Some("subagent-session-1")
        );
    }

    #[test]
    fn duplicate_agent_session_observations_are_idempotent() {
        let thread_id = WorkspaceThreadId::from("wt_a");
        let codex = HostBinding::new("codex", Some("profile_a".to_string()));
        let events = vec![
            ThreadEvent::created(thread_id.clone(), WorkspaceId::from("ws_a"), codex.clone()),
            ThreadEvent::agent_session_observed(
                thread_id.clone(),
                "codex",
                Some("profile_a".to_string()),
                "session-1",
            ),
            ThreadEvent::agent_session_observed(
                thread_id.clone(),
                "codex",
                Some("profile_a".to_string()),
                "session-1",
            ),
        ];

        let projection = ThreadProjection::from_events(&events).unwrap();
        let thread = projection.get(&thread_id).unwrap();

        assert_eq!(thread.agent_sessions.get(&codex).unwrap().len(), 1);
    }
}
