use super::*;

struct ActiveGeneration {
    id: u64,
    registry_id: u64,
    cancel_tx: watch::Sender<bool>,
    bridge_task: tokio::task::JoinHandle<()>,
}

struct StagedGeneration {
    id: u64,
    registry_id: u64,
    pipes: StdioPipes,
    cancel_tx: watch::Sender<bool>,
    cancel_rx: watch::Receiver<bool>,
}

pub(super) struct ProcessOwner {
    manager_tx: tokio::sync::mpsc::UnboundedSender<ManagerCommand>,
    process: Arc<SupervisedProcess>,
    registry: Arc<ChildRegistry>,
    spec: SpawnSpec,
    policy: RestartPolicy,
    factory: BridgeFactory,
    command_tx: tokio::sync::mpsc::UnboundedSender<ProcessCommand>,
    command_rx: tokio::sync::mpsc::UnboundedReceiver<ProcessCommand>,
    state: ProcessState,
    last_heartbeat_ts: u64,
    next_spawn_at: u64,
    next_generation: u64,
    consecutive_failures: u32,
    active: Option<ActiveGeneration>,
}

impl ProcessOwner {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        manager_tx: tokio::sync::mpsc::UnboundedSender<ManagerCommand>,
        process: Arc<SupervisedProcess>,
        registry: Arc<ChildRegistry>,
        spec: SpawnSpec,
        policy: RestartPolicy,
        factory: BridgeFactory,
        command_tx: tokio::sync::mpsc::UnboundedSender<ProcessCommand>,
        command_rx: tokio::sync::mpsc::UnboundedReceiver<ProcessCommand>,
        state: ProcessState,
    ) -> Self {
        Self {
            manager_tx,
            process,
            registry,
            spec,
            policy,
            factory,
            command_tx,
            command_rx,
            state,
            last_heartbeat_ts: now_secs(),
            next_spawn_at: 0,
            next_generation: 1,
            consecutive_failures: 0,
            active: None,
        }
    }

    pub(super) async fn run(mut self, start_immediately: bool) {
        if start_immediately {
            let _ = self.spawn_generation();
        }
        // A `Never` process that reached `Stopped` can never run again; the
        // owner must exit so its bridge factory (and any handshake channels
        // captured inside) is dropped.
        while !self.is_terminal() {
            let Some(command) = self.command_rx.recv().await else {
                break;
            };
            match command {
                ProcessCommand::Touch => {
                    self.last_heartbeat_ts = now_secs();
                    self.consecutive_failures = 0;
                }
                ProcessCommand::Tick(now) => self.tick(now).await,
                ProcessCommand::Start => self.start(),
                ProcessCommand::Stop { reason, reply } => {
                    self.stop(&reason).await;
                    let _ = reply.send(());
                }
                ProcessCommand::Shutdown { reason, reply } => {
                    self.stop(&reason).await;
                    let _ = reply.send(());
                    break;
                }
                ProcessCommand::Restart {
                    apply_backoff,
                    reply,
                } => {
                    let result = self.restart(apply_backoff).await;
                    let _ = reply.send(result);
                }
                ProcessCommand::BridgeExited {
                    generation_id,
                    registry_id,
                    exit,
                } => {
                    self.bridge_exited(generation_id, registry_id, exit).await;
                }
            }
        }
        self.stop_active().await;
    }

    fn is_terminal(&self) -> bool {
        matches!(self.policy, RestartPolicy::Never) && self.state.status == ProcessStatus::Stopped
    }

    async fn tick(&mut self, now: u64) {
        match self.state.status {
            ProcessStatus::NotStarted => {
                let _ = self.spawn_generation();
            }
            ProcessStatus::Crashed if self.next_spawn_at != 0 && now >= self.next_spawn_at => {
                let _ = self.spawn_generation();
            }
            ProcessStatus::Running => {
                if let Some(watchdog) = self.policy.watchdog() {
                    if now.saturating_sub(self.last_heartbeat_ts) > watchdog.as_secs() {
                        proc_log!(
                            info,
                            kind = self.process.kind,
                            label = self.process.label,
                            event = "watchdog_fired",
                            heartbeat_age_secs = now.saturating_sub(self.last_heartbeat_ts)
                        );
                        let _ = self.restart(true).await;
                    }
                }
            }
            _ => {}
        }
    }

    fn start(&mut self) {
        if self.active.is_some()
            || !matches!(
                self.state.status,
                ProcessStatus::Stopped | ProcessStatus::Crashed | ProcessStatus::NotStarted
            )
        {
            return;
        }
        self.consecutive_failures = 0;
        self.next_spawn_at = 0;
        self.set_state(ProcessStatus::Crashed, "started by user");
        let _ = self.spawn_generation();
    }

    async fn stop(&mut self, reason: &str) {
        self.next_spawn_at = 0;
        self.set_state(ProcessStatus::Stopped, reason);
        self.stop_active().await;
        self.remove_if_terminal();
    }

    async fn restart(&mut self, apply_backoff: bool) -> ProcessResult<()> {
        if self.state.status == ProcessStatus::Stopped
            && matches!(self.policy, RestartPolicy::Never)
        {
            return Ok(());
        }
        self.stop_active().await;
        let delay = if apply_backoff {
            self.next_restart_delay()
        } else {
            self.consecutive_failures = 0;
            Duration::ZERO
        };
        self.next_spawn_at = now_secs() + delay.as_secs();
        self.set_state(
            ProcessStatus::Crashed,
            if apply_backoff {
                "watchdog restart requested"
            } else {
                "restart requested"
            },
        );
        if delay.is_zero() {
            self.spawn_generation()
        } else {
            Ok(())
        }
    }

    fn spawn_generation(&mut self) -> ProcessResult<()> {
        if self.active.is_some()
            || !matches!(
                self.state.status,
                ProcessStatus::NotStarted | ProcessStatus::Crashed
            )
        {
            return Ok(());
        }
        self.next_spawn_at = 0;
        self.set_state(ProcessStatus::Spawning, "spawning");

        match self.spawn_child() {
            Ok(staged) => {
                let StagedGeneration {
                    id: generation_id,
                    registry_id,
                    pipes,
                    cancel_tx,
                    cancel_rx,
                } = staged;
                let bridge = (self.factory)();
                let command_tx = self.command_tx.clone();
                let bridge_task = tokio::spawn(async move {
                    let exit = bridge.run(pipes, cancel_rx).await;
                    let _ = command_tx.send(ProcessCommand::BridgeExited {
                        generation_id,
                        registry_id,
                        exit,
                    });
                });
                self.active = Some(ActiveGeneration {
                    id: generation_id,
                    registry_id,
                    cancel_tx,
                    bridge_task,
                });
                self.last_heartbeat_ts = now_secs();
                self.set_state(ProcessStatus::Running, "");
                proc_log!(
                    info,
                    kind = self.process.kind,
                    label = self.process.label,
                    event = "running"
                );
                Ok(())
            }
            Err(error) => {
                let reason = format!("spawn failed: {error}");
                proc_log!(
                    error,
                    kind = self.process.kind,
                    label = self.process.label,
                    event = "spawn_failed",
                    error = %reason
                );
                match self.policy {
                    RestartPolicy::Never => {
                        self.next_spawn_at = 0;
                        self.set_state(ProcessStatus::Stopped, reason);
                        self.remove_if_terminal();
                    }
                    RestartPolicy::OnCrash { .. } => {
                        self.next_spawn_at = now_secs() + self.next_restart_delay().as_secs();
                        self.set_state(ProcessStatus::Crashed, reason);
                    }
                }
                Err(error)
            }
        }
    }

    fn spawn_child(&mut self) -> ProcessResult<StagedGeneration> {
        let mut cmd: Command = env::command(&self.spec.program);
        cmd.args(&self.spec.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        if let Some(cwd) = &self.spec.cwd {
            cmd.current_dir(cwd);
        }
        for (key, value) in &self.spec.extra_env {
            cmd.env(key, value);
        }
        kill::prepare_tree_root(&mut cmd);

        let mut child = cmd.spawn().map_err(|source| ProcessError::Spawn {
            program: self.spec.program.clone(),
            source,
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or(ProcessError::StdioUnavailable { what: "stdin" })?;
        let stdout = child
            .stdout
            .take()
            .ok_or(ProcessError::StdioUnavailable { what: "stdout" })?;
        let stderr_raw = child
            .stderr
            .take()
            .ok_or(ProcessError::StdioUnavailable { what: "stderr" })?;
        let pid = child.id();
        let registry_id =
            self.registry
                .register(self.process.kind, self.process.label.clone(), child);
        let generation_id = self.next_generation;
        self.next_generation = self.next_generation.wrapping_add(1);

        proc_log!(
            info,
            kind = self.process.kind,
            label = self.process.label,
            pid = pid,
            event = "spawned",
            program = %self.spec.program
        );

        let stderr = if self.spec.capture_stderr {
            Some(stderr_raw)
        } else {
            let kind = self.process.kind;
            let label = self.process.label.clone();
            tokio::spawn(async move {
                use tokio::io::AsyncBufReadExt;
                let reader = tokio::io::BufReader::new(stderr_raw);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    proc_log!(info, kind = kind, label = label, event = "stderr", line = %line);
                }
            });
            None
        };
        let (cancel_tx, cancel_rx) = watch::channel(false);
        Ok(StagedGeneration {
            id: generation_id,
            registry_id,
            pipes: StdioPipes {
                stdin,
                stdout,
                stderr,
            },
            cancel_tx,
            cancel_rx,
        })
    }

    async fn bridge_exited(&mut self, generation_id: u64, registry_id: u64, exit: BridgeExit) {
        if self
            .active
            .as_ref()
            .is_none_or(|generation| generation.id != generation_id)
        {
            return;
        }
        self.active.take();
        let child_status = self.terminate_and_reap(registry_id).await;
        let (reason, was_crash) = match exit {
            BridgeExit::Clean => match child_status {
                Some(status) if status.success() => {
                    (format!("process exited successfully ({status})"), false)
                }
                Some(status) => (format!("process exited with {status}"), true),
                None => ("clean bridge exit".to_string(), false),
            },
            BridgeExit::Cancelled => ("cancelled".to_string(), false),
            BridgeExit::ProtocolError(error) => (format!("protocol error: {error}"), true),
        };

        match self.policy {
            RestartPolicy::Never => {
                self.next_spawn_at = 0;
                self.set_state(ProcessStatus::Stopped, &reason);
                proc_log!(
                    info,
                    kind = self.process.kind,
                    label = self.process.label,
                    event = if was_crash { "exited_no_restart" } else { "exited" },
                    reason = %reason
                );
                self.remove_if_terminal();
            }
            RestartPolicy::OnCrash { .. } => {
                let delay = self.next_restart_delay();
                self.next_spawn_at = now_secs() + delay.as_secs();
                self.set_state(ProcessStatus::Crashed, &reason);
                proc_log!(
                    warn,
                    kind = self.process.kind,
                    label = self.process.label,
                    event = "crashed",
                    reason = %reason,
                    respawn_in_secs = delay.as_secs()
                );
            }
        }
    }

    async fn stop_active(&mut self) {
        let Some(mut generation) = self.active.take() else {
            return;
        };
        let _ = generation.cancel_tx.send(true);
        self.terminate_and_reap(generation.registry_id).await;
        if tokio::time::timeout(BRIDGE_SHUTDOWN_TIMEOUT, &mut generation.bridge_task)
            .await
            .is_err()
        {
            proc_log!(
                warn,
                kind = self.process.kind,
                label = self.process.label,
                event = "bridge_shutdown_timeout",
                generation = generation.id
            );
            generation.bridge_task.abort();
            let _ = generation.bridge_task.await;
        }
    }

    async fn terminate_and_reap(&mut self, registry_id: u64) -> Option<std::process::ExitStatus> {
        let mut child = self.registry.remove(registry_id)?;
        let root_pid = child.id();
        let observed_status =
            match kill::wait_for_exit_within(&mut child, CHILD_EXIT_OBSERVATION_TIMEOUT).await {
                Ok(status) => status,
                Err(error) => {
                    proc_log!(
                        warn,
                        kind = self.process.kind,
                        label = self.process.label,
                        event = "child_status_read_failed",
                        error = %error
                    );
                    None
                }
            };
        if let Err(error) = kill::terminate_child_tree(&mut child, root_pid).await {
            proc_log!(
                warn,
                kind = self.process.kind,
                label = self.process.label,
                event = "child_tree_terminate_failed",
                error = %error
            );
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
        observed_status
    }

    fn next_restart_delay(&mut self) -> Duration {
        let Some(base) = self.policy.restart_delay() else {
            return Duration::ZERO;
        };
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        restart_delay_for_failure(base, self.consecutive_failures)
    }

    fn set_state(&mut self, status: ProcessStatus, reason: impl Into<String>) {
        self.state = ProcessState {
            status,
            reason: reason.into(),
        };
        self.process.state_tx.send_replace(self.state.clone());
        let _ = self
            .manager_tx
            .send(ManagerCommand::ProcessChanged(Arc::clone(&self.process)));
    }

    fn remove_if_terminal(&self) {
        if matches!(self.policy, RestartPolicy::Never)
            && self.state.status == ProcessStatus::Stopped
        {
            let _ = self
                .manager_tx
                .send(ManagerCommand::Remove(Arc::clone(&self.process)));
        }
    }
}

pub(super) fn restart_delay_for_failure(base: Duration, failure: u32) -> Duration {
    let exponent = failure.saturating_sub(1).min(16);
    base.saturating_mul(1_u32 << exponent)
        .min(MAX_RESTART_DELAY)
}
