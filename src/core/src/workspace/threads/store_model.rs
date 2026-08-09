use super::*;

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
    #[allow(clippy::too_many_arguments)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_thread_id: Option<WorkspaceThreadId>,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_thread_id: Option<WorkspaceThreadId>,
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
        parent_thread_id: Option<WorkspaceThreadId>,
        host_binding: HostBinding,
    ) -> Self {
        Self::Created {
            schema_version: SCHEMA_VERSION,
            event_id: event_id(),
            occurred_at: now(),
            thread_id: thread_id.into(),
            workspace_id: workspace_id.into(),
            parent_thread_id,
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
    ) -> Self {
        Self::HostChanged {
            schema_version: SCHEMA_VERSION,
            event_id: event_id(),
            occurred_at: now(),
            thread_id: thread_id.into(),
            host_binding,
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
