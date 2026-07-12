use super::*;

#[derive(Clone, Default)]
pub(super) struct TurnState {
    pub(super) busy: bool,
    pub(super) failed: Option<String>,
    pub(super) session_id: Option<String>,
    pub(super) host_agent: Option<Arc<Agent>>,
}

pub(super) struct PromptCommand {
    pub(super) runtime: Arc<ThreadRuntime>,
    pub(super) target: ChannelTarget,
    pub(super) content_blocks: Vec<acp::ContentBlock>,
    pub(super) handler: Arc<dyn AgentClientHandler>,
    pub(super) cancellation: Option<PromptCancellation>,
    pub(super) reply: oneshot::Sender<acp::Result<acp::PromptResponse>>,
}

pub(super) struct StartCommand {
    pub(super) runtime: Arc<ThreadRuntime>,
    pub(super) route: RouteKey,
    pub(super) handler: Arc<dyn AgentClientHandler>,
    pub(super) reply: oneshot::Sender<acp::Result<String>>,
}

pub(super) struct RuntimeCommand<T> {
    pub(super) runtime: Arc<ThreadRuntime>,
    pub(super) reply: oneshot::Sender<T>,
}

pub(super) struct CloseCommand {
    pub(super) runtime: Arc<ThreadRuntime>,
    pub(super) reason: Option<String>,
    pub(super) reply: oneshot::Sender<acp::Result<()>>,
}

pub(super) struct SwitchProfileCommand {
    pub(super) runtime: Arc<ThreadRuntime>,
    pub(super) host_binding: HostBinding,
    pub(super) reply: oneshot::Sender<acp::Result<()>>,
}

pub(super) enum ThreadOwnerCommand {
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

pub(super) struct ThreadOwner {
    pub(super) command_rx: mpsc::UnboundedReceiver<ThreadOwnerCommand>,
    pub(super) state_tx: watch::Sender<TurnState>,
    pub(super) change_tx: Option<broadcast::Sender<()>>,
    pub(super) host: Option<AcpSessionRunner>,
    pub(super) session_id: Option<String>,
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

    pub(super) async fn cancelled(&mut self) {
        tokio::select! {
            _ = wait_for_signal(&mut self.cancel_rx) => {}
            _ = wait_for_signal(&mut self.shutdown_rx) => {}
        }
    }
}

impl ThreadOwner {
    pub(super) async fn run(mut self) {
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
