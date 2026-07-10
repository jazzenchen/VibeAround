use super::*;

impl Supervisor {
    pub(super) fn schedule_spawn(self: &Arc<Self>, proc: Arc<SupervisedProcess>) {
        let mut slot = proc.spawn_task.lock();
        if slot.as_ref().is_some_and(|task| !task.is_finished()) {
            return;
        }
        let sup = Arc::clone(self);
        let proc_for_task = Arc::clone(&proc);
        *slot = Some(tokio::spawn(async move {
            sup.begin_spawn(proc_for_task).await;
        }));
    }

    pub(super) async fn run_tick_loop(self: Arc<Self>) {
        let mut ticker = tokio::time::interval(TICK_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        ticker.tick().await; // consume the immediate first tick

        tracing::info!(
            tick_secs = TICK_INTERVAL.as_secs(),
            "supervisor loop started"
        );
        loop {
            ticker.tick().await;
            self.tick().await;
        }
    }

    pub(super) async fn tick(self: &Arc<Self>) {
        let now = now_secs();
        let mut to_spawn: Vec<Arc<SupervisedProcess>> = Vec::new();
        let mut to_watchdog: Vec<Arc<SupervisedProcess>> = Vec::new();

        for proc in self.processes.read().values().cloned() {
            match proc.status() {
                ProcessStatus::NotStarted => to_spawn.push(proc),
                ProcessStatus::Crashed => {
                    let at = proc.next_spawn_at.load(Ordering::Relaxed);
                    if at != 0 && now >= at {
                        to_spawn.push(proc);
                    }
                }
                ProcessStatus::Running => {
                    if let Some(watchdog) = proc.policy.watchdog() {
                        let last = proc.last_heartbeat_ts.load(Ordering::Relaxed);
                        if now.saturating_sub(last) > watchdog.as_secs() {
                            to_watchdog.push(proc);
                        }
                    }
                }
                ProcessStatus::Spawning | ProcessStatus::Stopped => {}
            }
        }

        for proc in to_watchdog {
            let age = now.saturating_sub(proc.last_heartbeat_ts.load(Ordering::Relaxed));
            proc_log!(
                info,
                kind = proc.kind,
                label = proc.label,
                event = "watchdog_fired",
                heartbeat_age_secs = age
            );
            self.cancel_current_bridge(&proc);
        }

        for proc in to_spawn {
            self.schedule_spawn(proc);
        }
    }

    pub(super) async fn begin_spawn(self: &Arc<Self>, proc: Arc<SupervisedProcess>) {
        // Guard against racing tickers / immediate-spawn.
        {
            let _transition = proc.transition_lock.lock();
            if !matches!(
                proc.status(),
                ProcessStatus::NotStarted | ProcessStatus::Crashed
            ) {
                return;
            }
            proc.set_status(ProcessStatus::Spawning);
            proc.next_spawn_at.store(0, Ordering::Relaxed);
            proc.set_reason("spawning");
        }
        self.notify_change(&proc);

        match self.spawn_child(&proc).await {
            Ok((pipes, cancel_rx)) => {
                // Publish status Running unless force_* landed during the spawn.
                let publish_running = {
                    let _transition = proc.transition_lock.lock();
                    if matches!(proc.status(), ProcessStatus::Spawning) {
                        proc.set_status(ProcessStatus::Running);
                        proc.set_reason("");
                        proc.last_heartbeat_ts.store(now_secs(), Ordering::Relaxed);
                        true
                    } else {
                        false
                    }
                };
                if !publish_running {
                    proc_log!(
                        info,
                        kind = proc.kind,
                        label = proc.label,
                        event = "spawn_superseded"
                    );
                    // No bridge task has been started yet, so cancellation alone
                    // cannot clean up this staged generation.
                    *proc.cancel_tx.write() = None;
                    self.terminate_and_reap_current_child(&proc).await;
                    return;
                }
                proc_log!(
                    info,
                    kind = proc.kind,
                    label = proc.label,
                    event = "running"
                );
                self.notify_change(&proc);

                // Hand pipes to the bridge in a task; when it returns, we
                // transition based on intent.
                let bridge = (proc.factory)();
                let sup = Arc::clone(self);
                let proc_for_task = Arc::clone(&proc);
                tokio::spawn(async move {
                    let exit = bridge.run(pipes, cancel_rx).await;
                    sup.handle_bridge_exit(proc_for_task, exit).await;
                });
            }
            Err(e) => {
                let reason = format!("{}", e);
                let publish_crash = {
                    let _transition = proc.transition_lock.lock();
                    if matches!(proc.status(), ProcessStatus::Spawning) {
                        proc.set_status(ProcessStatus::Crashed);
                        proc.set_reason(format!("spawn failed: {}", reason));
                        if let Some(delay) = proc.policy.restart_delay() {
                            proc.next_spawn_at
                                .store(now_secs() + delay.as_secs(), Ordering::Relaxed);
                        }
                        true
                    } else {
                        false
                    }
                };
                if !publish_crash {
                    proc_log!(
                        error,
                        kind = proc.kind,
                        label = proc.label,
                        event = "spawn_failed_superseded",
                        error = %reason
                    );
                    return;
                }
                proc_log!(
                    error,
                    kind = proc.kind,
                    label = proc.label,
                    event = "spawn_failed",
                    error = %reason
                );
                self.notify_change(&proc);
            }
        }
    }

    pub(super) async fn spawn_child(
        &self,
        proc: &SupervisedProcess,
    ) -> ProcessResult<(StdioPipes, watch::Receiver<bool>)> {
        let mut cmd: Command = env::command(&proc.spec.program);
        cmd.args(&proc.spec.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        if let Some(cwd) = &proc.spec.cwd {
            cmd.current_dir(cwd);
        }
        for (k, v) in &proc.spec.extra_env {
            cmd.env(k, v);
        }
        kill::prepare_tree_root(&mut cmd);

        let mut child = cmd.spawn().map_err(|e| ProcessError::Spawn {
            program: proc.spec.program.clone(),
            source: e,
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

        // Hand ownership of the Child to the global registry. This is the
        // canonical owner — kill_on_drop alone can't be relied on under
        // abrupt runtime teardown. The id is stashed on the proc so
        // `handle_bridge_exit` can remove + reap the child (otherwise
        // every respawn leaks a registry entry + zombie process).
        let registry_id = self.registry.register(proc.kind, proc.label.clone(), child);
        *proc.current_registry_id.lock() = Some(registry_id);

        proc_log!(
            info,
            kind = proc.kind,
            label = proc.label,
            pid = pid,
            event = "spawned",
            program = %proc.spec.program
        );

        let stderr = if proc.spec.capture_stderr {
            Some(stderr_raw)
        } else {
            let kind = proc.kind;
            let label = proc.label.clone();
            tokio::spawn(async move {
                use tokio::io::AsyncBufReadExt;
                let reader = tokio::io::BufReader::new(stderr_raw);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    proc_log!(
                        info,
                        kind = kind,
                        label = label,
                        event = "stderr",
                        line = %line
                    );
                }
            });
            None
        };

        let (cancel_tx, cancel_rx) = watch::channel(false);
        *proc.cancel_tx.write() = Some(cancel_tx);

        Ok((
            StdioPipes {
                stdin,
                stdout,
                stderr,
            },
            cancel_rx,
        ))
    }

    pub(super) async fn handle_bridge_exit(
        self: Arc<Self>,
        proc: Arc<SupervisedProcess>,
        exit: BridgeExit,
    ) {
        // Clear the cancel channel — this run is done.
        *proc.cancel_tx.write() = None;

        // A bridge can finish while its child is frozen or otherwise still
        // alive. Terminate that generation before waiting so respawn cannot
        // leave a twin process behind, then reap before publishing the next
        // state transition.
        self.terminate_and_reap_current_child(&proc).await;

        let (reason, was_crash) = match exit {
            BridgeExit::Clean => ("clean exit".to_string(), false),
            BridgeExit::Cancelled => ("cancelled".to_string(), false),
            BridgeExit::ProtocolError(e) => (format!("protocol error: {}", e), true),
        };

        let transition = proc.transition_lock.lock();
        if matches!(proc.status(), ProcessStatus::Stopped) {
            // `force_stop` won while this generation was being reaped.
            proc.intent
                .store(TransitionIntent::None as u8, Ordering::Release);
            proc.next_spawn_at.store(0, Ordering::Relaxed);
            drop(transition);
            self.deregister_terminal_process(&proc);
            return;
        }

        // Atomically consume intent so two callers don't both observe Stop.
        let intent = TransitionIntent::from_u8(
            proc.intent
                .swap(TransitionIntent::None as u8, Ordering::AcqRel),
        );

        match intent {
            TransitionIntent::Stop => {
                proc.set_status(ProcessStatus::Stopped);
                proc.next_spawn_at.store(0, Ordering::Relaxed);
                proc.set_reason(&reason);
                proc_log!(
                    info,
                    kind = proc.kind,
                    label = proc.label,
                    event = "stopped",
                    reason = %reason
                );
            }
            TransitionIntent::Restart => {
                proc.set_status(ProcessStatus::Crashed);
                proc.next_spawn_at.store(now_secs(), Ordering::Relaxed);
                proc.set_reason(&reason);
                proc_log!(
                    info,
                    kind = proc.kind,
                    label = proc.label,
                    event = "restart_requested",
                    reason = %reason
                );
            }
            TransitionIntent::None => match proc.policy {
                RestartPolicy::Never => {
                    proc.set_status(ProcessStatus::Stopped);
                    proc.next_spawn_at.store(0, Ordering::Relaxed);
                    proc.set_reason(&reason);
                    if was_crash {
                        proc_log!(
                            warn,
                            kind = proc.kind,
                            label = proc.label,
                            event = "exited_no_restart",
                            reason = %reason
                        );
                    } else {
                        proc_log!(
                            info,
                            kind = proc.kind,
                            label = proc.label,
                            event = "exited",
                            reason = %reason
                        );
                    }
                }
                RestartPolicy::OnCrash { .. } => {
                    proc.set_status(ProcessStatus::Crashed);
                    let delay = proc.policy.restart_delay().unwrap_or(Duration::ZERO);
                    proc.next_spawn_at
                        .store(now_secs() + delay.as_secs(), Ordering::Relaxed);
                    proc.set_reason(&reason);
                    proc_log!(
                        warn,
                        kind = proc.kind,
                        label = proc.label,
                        event = "crashed",
                        reason = %reason,
                        respawn_in_secs = delay.as_secs()
                    );
                }
            },
        }
        drop(transition);
        self.notify_change(&proc);
        self.deregister_terminal_process(&proc);
    }

    pub(super) async fn terminate_and_reap_current_child(&self, proc: &SupervisedProcess) {
        let Some(id) = proc.current_registry_id.lock().take() else {
            return;
        };
        let Some(mut child) = self.registry.remove(id) else {
            return;
        };
        let root_pid = child.id();

        if let Err(error) = kill::terminate_child_tree(&mut child, root_pid).await {
            proc_log!(
                warn,
                kind = proc.kind,
                label = proc.label,
                event = "child_tree_terminate_failed",
                error = %error
            );
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
    }

    pub(super) fn deregister_terminal_process(&self, proc: &SupervisedProcess) {
        // Auto-deregister terminal one-shot processes. Without this the
        // `processes` map grows unbounded over daemon lifetime as
        // `RestartPolicy::Never` workloads (chiefly `AcpAgent` spawns
        // tied to one-shot agent launches) accumulate Stopped entries.
        // Keeping `OnCrash` entries around is deliberate: a user-stopped
        // channel plugin can still be resurrected via `force_start`.
        if matches!(proc.policy, RestartPolicy::Never)
            && matches!(proc.status(), ProcessStatus::Stopped)
            && self.processes.write().remove(&proc.id).is_some()
        {
            proc_log!(
                info,
                kind = proc.kind,
                label = proc.label,
                event = "deregistered"
            );
        }
    }
}
