use super::*;

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
                parent_thread_id,
                preview_slug,
                host_binding,
                ..
            } => self.create(
                thread_id.clone(),
                workspace_id.clone(),
                parent_thread_id.clone(),
                preview_slug.clone(),
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
        parent_thread_id: Option<WorkspaceThreadId>,
        preview_slug: Option<String>,
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
                parent_thread_id,
                preview_slug,
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
