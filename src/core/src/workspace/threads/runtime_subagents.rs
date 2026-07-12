use super::*;

impl ThreadRuntime {
    pub async fn initialize_multi_agent_turn(
        &self,
        turn: MultiAgentTurn,
        agents: Vec<ThreadAgent>,
    ) -> acp::Result<()> {
        self.mark_activity();
        if self.thread_snapshot().status == ThreadStatus::Closed {
            return Err(acp::Error::new(-32603, "workspace thread is closed"));
        }

        let thread_id = self.thread_snapshot().id;
        let event = ThreadEvent::multi_agent_turn_initialized(thread_id, turn, agents);
        append_thread_event(&self.store, &event).await?;
        self.apply_thread_event(&event).await?;
        self.notify_change();
        Ok(())
    }

    pub async fn recover_interrupted_subagents(&self) -> acp::Result<Vec<ThreadAgent>> {
        let interrupted_ids = {
            let thread = self.thread_snapshot();
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
        if self.thread_snapshot().status == ThreadStatus::Closed {
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
        self.register_subagent(
            thread_agent.id.clone(),
            SubagentRuntime {
                agent: Arc::clone(&agent),
                session_id: session_id.clone(),
                client_handler: Arc::clone(&runtime_handler),
                active_turn_target: active_turn_target.clone(),
                completion_validator: completion_validator_for_runtime,
            },
        )
        .await?;

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
        if self.thread_snapshot().status == ThreadStatus::Closed {
            return Err(acp::Error::new(-32603, "workspace thread is closed"));
        }

        let thread_agent = {
            let thread = self.thread_snapshot();
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

        let Some(subagent) = self.turn_state.borrow().subagents.get(agent_id).cloned() else {
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

    async fn register_subagent(
        &self,
        agent_id: ThreadAgentId,
        subagent: SubagentRuntime,
    ) -> acp::Result<()> {
        let (reply, done) = oneshot::channel();
        self.owner_tx
            .send(ThreadOwnerCommand::RegisterSubagent(Box::new(
                RegisterSubagentCommand {
                    agent_id,
                    subagent,
                    reply,
                },
            )))
            .map_err(|_| runtime_stopped_error())?;
        done.await.unwrap_or_else(|_| Err(runtime_stopped_error()))
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
        let target_guard = active_turn_target.install(target);
        let mut cancellation = target_guard.cancellation();
        let agent_id = thread_agent.id.clone();
        for attempt in 1..=SUBAGENT_PROMPT_MAX_ATTEMPTS {
            if let Some(validator) = completion_validator.as_ref() {
                validator.reset_completion().await;
            }

            let prompt_request = acp::PromptRequest::new(
                session_id.clone(),
                vec![acp::ContentBlock::Text(acp::TextContent::new(
                    prompt.clone(),
                ))],
            );
            let prompt_call = agent.prompt(prompt_request);
            tokio::pin!(prompt_call);
            let mut cancelled = false;
            let result = tokio::select! {
                _ = wait_for_signal(&mut cancellation) => {
                    cancelled = true;
                    let _ = agent
                        .cancel(acp::CancelNotification::new(session_id.clone()))
                        .await;
                    Err(acp::Error::new(-32800, "subagent prompt cancelled"))
                }
                result = &mut prompt_call => result,
            };
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
                    if cancelled {
                        self.set_subagent_error(&agent_id, message, &status_tx)
                            .await;
                        return;
                    }
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
        let thread = self.thread_snapshot();
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
}
