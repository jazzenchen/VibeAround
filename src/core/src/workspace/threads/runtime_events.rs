use super::*;

impl ThreadRuntime {
    pub(super) async fn set_thread_agent_status(
        &self,
        agent_id: &ThreadAgentId,
        status: ThreadAgentStatus,
        last_error: Option<String>,
        report: Option<serde_json::Value>,
    ) -> acp::Result<Option<ThreadAgent>> {
        self.set_thread_agent_status_with_session(agent_id, status, None, last_error, report)
            .await
    }

    pub(super) async fn set_thread_agent_status_with_session(
        &self,
        agent_id: &ThreadAgentId,
        status: ThreadAgentStatus,
        session_id: Option<String>,
        last_error: Option<String>,
        report: Option<serde_json::Value>,
    ) -> acp::Result<Option<ThreadAgent>> {
        let thread_id = self.thread_snapshot().id;
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
        let thread = self.thread_snapshot();
        Ok(thread.agents.get(agent_id).cloned())
    }

    pub(super) async fn apply_thread_event(&self, event: &ThreadEvent) -> acp::Result<()> {
        let (reply, done) = oneshot::channel();
        self.owner_tx
            .send(ThreadOwnerCommand::ApplyThreadEvent {
                event: Box::new(event.clone()),
                reply,
            })
            .map_err(|_| runtime_stopped_error())?;
        done.await.unwrap_or_else(|_| Err(runtime_stopped_error()))
    }
}

pub(super) fn apply_thread_event_to(thread: &mut WorkspaceThread, event: &ThreadEvent) {
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
            let session = super::super::store::AgentSessionRef {
                agent_id: agent_id.clone(),
                profile_id: profile_id.clone(),
                session_id: session_id.clone(),
                observed_at: occurred_at.clone(),
            };
            if thread.has_agent_session(&session.binding(), &session.session_id) {
                return;
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
            if !super::super::store::closed_reason_closes_thread(reason.as_deref()) {
                return;
            }
            thread.status = ThreadStatus::Closed;
            thread.updated_at = occurred_at.clone();
        }
        ThreadEvent::Created { .. } => {}
    }
}
