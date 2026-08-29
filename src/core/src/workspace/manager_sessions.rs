use super::*;

impl WorkspaceThreadManager {
    pub async fn attach_external_session(
        &self,
        route: &RouteKey,
        agent_id: String,
        profile_id: Option<String>,
        session_id: String,
        cwd: PathBuf,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        let prepared = self
            .prepare_external_session_thread(agent_id, profile_id, session_id, cwd)
            .await?;
        let thread = self.ensure_external_session_thread(&prepared).await?;
        if self.current_attachment(route).await?.is_some() {
            self.detach_route(route).await?;
        }
        self.attach_route(
            route.clone(),
            prepared.workspace.id.clone(),
            thread.id.clone(),
        )
        .await?;
        self.runtime_from_thread(thread).await
    }

    pub async fn attach_external_session_to_web_thread(
        &self,
        agent_id: String,
        profile_id: Option<String>,
        session_id: String,
        cwd: PathBuf,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        let prepared = self
            .prepare_external_session_thread(agent_id, profile_id, session_id, cwd)
            .await?;
        let route = web_route_for_thread(&prepared.thread.id);
        let thread = self.ensure_external_session_thread(&prepared).await?;
        self.attach_route(route, prepared.workspace.id.clone(), thread.id.clone())
            .await?;
        self.runtime_from_thread(thread).await
    }

    /// Give an existing thread its own web route, whoever created it. The web
    /// chat id is the thread id, so this is what makes that name true rather
    /// than letting the browser assume it.
    pub async fn attach_web_route_to_thread(
        &self,
        thread_id: &WorkspaceThreadId,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        let thread = self
            .thread(thread_id)
            .await?
            .ok_or_else(|| anyhow!("thread {} not found", thread_id))?;
        let route = web_route_for_thread(&thread.id);
        self.attach_route(route, thread.workspace_id.clone(), thread.id.clone())
            .await?;
        self.runtime_from_thread(thread).await
    }

    /// Promise the web a thread without creating one. Nothing is written and no
    /// agent is contacted; the record waits on its own route until a first
    /// prompt arrives, and a conversation the user never starts leaves no
    /// trace. Answers with the record and the workspace it would live in.
    pub async fn draft_web_thread(
        &self,
        agent_id: String,
        profile_id: Option<String>,
        cwd: PathBuf,
    ) -> anyhow::Result<(WorkspaceThread, PathBuf)> {
        let profile_id = normalize_optional_launch_profile_id(profile_id.as_deref())
            .or_else(|| launch_setting_profile_for_agent(&agent_id));
        let workspace = self.ensure_workspace_for_cwd(cwd).await?;
        let host_binding = HostBinding::new(agent_id, profile_id);
        let thread = self.new_thread_record_with_host(workspace.id.clone(), None, host_binding);
        self.drafts
            .lock()
            .await
            .insert(web_route_for_thread(&thread.id), thread.clone());
        Ok((thread, workspace.cwd))
    }

    pub async fn switch_workspace(
        &self,
        route: &RouteKey,
        token: &str,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        let workspace = self
            .resolve_workspace(token)
            .await?
            .ok_or_else(|| anyhow!("workspace '{}' not found", token))?;
        self.create_thread_for_route(route, workspace.id).await
    }

    pub async fn list_workspaces(&self) -> anyhow::Result<Vec<WorkspaceRecord>> {
        Ok(self
            .workspace_projection()
            .await?
            .active()
            .cloned()
            .collect())
    }

    pub async fn list_resumable_agent_sessions(
        &self,
        agent_id: &str,
        workspace: &Path,
        limit: usize,
        include_archived: bool,
    ) -> anyhow::Result<Vec<crate::launch_sessions::LaunchSession>> {
        let workspace = normalize_workspace_cwd(workspace);
        let excluded = self
            .subagent_session_ids_for_agent_workspace(agent_id, &workspace)
            .await?;
        let sessions = crate::launch_sessions::list_native_for_agent_workspace_with_archived_async(
            agent_id,
            &workspace,
            usize::MAX,
            include_archived,
        )
        .await
        .into_iter()
        .filter(|session| !excluded.contains(&session.session_id))
        .take(limit)
        .collect();
        Ok(sessions)
    }

    pub async fn thread_id_for_agent_session(
        &self,
        agent_id: &str,
        workspace: &Path,
        session_id: &str,
    ) -> anyhow::Result<Option<WorkspaceThreadId>> {
        Ok(self
            .thread_host_for_agent_session(agent_id, workspace, session_id)
            .await?
            .map(|(thread_id, _)| thread_id))
    }

    pub async fn thread_host_for_agent_session(
        &self,
        agent_id: &str,
        workspace: &Path,
        session_id: &str,
    ) -> anyhow::Result<Option<(WorkspaceThreadId, HostBinding)>> {
        let workspace = normalize_workspace_cwd(workspace);
        let workspace_projection = self.workspace_projection().await?;
        let Some(workspace) = workspace_by_cwd(&workspace_projection, &workspace) else {
            return Ok(None);
        };
        let thread_projection = self.thread_projection().await?;
        Ok(thread_projection
            .for_workspace(&workspace.id, true)
            .filter(|thread| {
                thread
                    .agent_sessions
                    .values()
                    .flatten()
                    .any(|session| session.agent_id == agent_id && session.session_id == session_id)
            })
            .max_by(|a, b| a.updated_at.cmp(&b.updated_at))
            .map(|thread| (thread.id.clone(), thread.host_binding.clone())))
    }

    pub async fn subagent_session_ids_for_agent_workspace(
        &self,
        agent_id: &str,
        cwd: &Path,
    ) -> anyhow::Result<HashSet<String>> {
        let cwd = normalize_workspace_cwd(cwd);
        let workspace_projection = self.workspace_projection().await?;
        let Some(workspace) = workspace_by_cwd(&workspace_projection, &cwd) else {
            return Ok(HashSet::new());
        };
        let thread_projection = self.thread_projection().await?;
        Ok(thread_projection
            .for_workspace(&workspace.id, true)
            .flat_map(|thread| thread.agents.values())
            .filter(|agent| agent.agent_id == agent_id)
            .filter_map(|agent| agent.session_id.clone())
            .collect())
    }

    async fn resolve_external_session_id(
        &self,
        projection: &ThreadProjection,
        workspace_id: &WorkspaceId,
        workspace: &Path,
        host_binding: &HostBinding,
        requested_session_id: &str,
    ) -> anyhow::Result<String> {
        let requested_session_id = requested_session_id.trim();
        if requested_session_id.is_empty() {
            return Ok(String::new());
        }

        if let Some(session_id) = resolve_thread_session_alias(
            projection,
            workspace_id,
            host_binding,
            requested_session_id,
        )? {
            return Ok(session_id);
        }

        let matches = crate::launch_sessions::list_native_for_agent_workspace_with_archived_async(
            &host_binding.agent_id,
            workspace,
            usize::MAX,
            false,
        )
        .await
        .into_iter()
        .filter(|session| session_id_matches(&session.session_id, requested_session_id))
        .map(|session| session.session_id)
        .collect::<Vec<_>>();

        match unique_session_match(matches, requested_session_id)? {
            Some(session_id) => Ok(session_id),
            None => Ok(requested_session_id.to_string()),
        }
    }

    async fn prepare_external_session_thread(
        &self,
        agent_id: String,
        profile_id: Option<String>,
        session_id: String,
        cwd: PathBuf,
    ) -> anyhow::Result<PreparedExternalSessionThread> {
        let workspace = self.ensure_workspace_for_cwd(cwd).await?;
        let projection = self.thread_projection().await?;
        let explicit_profile_id = normalize_optional_launch_profile_id(profile_id.as_deref());
        let launch_setting_profile_id = launch_setting_profile_for_agent(&agent_id);
        let alias_binding = HostBinding::new(
            agent_id.clone(),
            explicit_profile_id
                .clone()
                .or_else(|| launch_setting_profile_id.clone()),
        );
        let session_id = self
            .resolve_external_session_id(
                &projection,
                &workspace.id,
                &workspace.cwd,
                &alias_binding,
                &session_id,
            )
            .await?;
        let profile_id = match explicit_profile_id {
            Some(profile_id) => Some(profile_id),
            None => self
                .profile_id_for_agent_session(
                    &projection,
                    &workspace.id,
                    &workspace.cwd,
                    &agent_id,
                    &session_id,
                )
                .await?
                .or(launch_setting_profile_id),
        };
        let host_binding = HostBinding::new(agent_id.clone(), profile_id.clone());
        let session_seen_in_thread_store =
            projection.for_workspace(&workspace.id, true).any(|thread| {
                thread
                    .agent_sessions
                    .values()
                    .flatten()
                    .any(|session| session.agent_id == agent_id && session.session_id == session_id)
            });
        // A native session belongs to at most one open thread: resuming a
        // session that an open thread already holds joins that thread
        // (subscription), it never mints a sibling. Splitting one session
        // across thread ids is what made per-surface conversations bleed
        // into each other after a cold restart.
        // TODO: Revisit persisted thread lifecycle states here. "Closed" can
        // mean user-ended, while Web idle only unloads the runtime.
        let thread = projection
            .for_workspace(&workspace.id, false)
            .find(|thread| {
                thread.status != ThreadStatus::Closed
                    && thread.agent_sessions.values().flatten().any(|session| {
                        session.agent_id == agent_id && session.session_id == session_id
                    })
            })
            .cloned();
        let thread = if let Some(thread) = thread {
            thread
        } else {
            let session_exists = session_seen_in_thread_store
                || crate::launch_sessions::list_native_for_agent_workspace_with_archived_async(
                    &agent_id,
                    &workspace.cwd,
                    usize::MAX,
                    false,
                )
                .await
                .into_iter()
                .any(|session| session.session_id == session_id);
            if !session_exists {
                return Err(anyhow!(
                    "session '{}' was not found for agent '{}' in workspace {}",
                    session_id,
                    agent_id,
                    workspace.cwd.to_string_lossy()
                ));
            }
            self.new_thread_record_with_host(workspace.id.clone(), None, host_binding.clone())
        };

        Ok(PreparedExternalSessionThread {
            workspace,
            host_binding,
            agent_id,
            profile_id,
            session_id,
            thread,
        })
    }

    async fn profile_id_for_agent_session(
        &self,
        projection: &ThreadProjection,
        workspace_id: &WorkspaceId,
        workspace: &Path,
        agent_id: &str,
        session_id: &str,
    ) -> anyhow::Result<Option<String>> {
        if let Some(profile_id) =
            projection
                .for_workspace(workspace_id, true)
                .filter(|thread| {
                    thread.agent_sessions.values().flatten().any(|session| {
                        session.agent_id == agent_id && session.session_id == session_id
                    })
                })
                .max_by(|a, b| a.updated_at.cmp(&b.updated_at))
                .and_then(|thread| {
                    if thread.host_binding.agent_id == agent_id {
                        thread.host_binding.profile_id.clone()
                    } else {
                        thread
                            .agent_sessions
                            .values()
                            .flatten()
                            .filter(|session| {
                                session.agent_id == agent_id && session.session_id == session_id
                            })
                            .max_by(|a, b| a.observed_at.cmp(&b.observed_at))
                            .and_then(|session| session.profile_id.clone())
                    }
                })
        {
            return Ok(Some(profile_id));
        }

        Ok(
            crate::launch_sessions::list_native_for_agent_workspace_with_archived_async(
                agent_id,
                workspace,
                usize::MAX,
                false,
            )
            .await
            .into_iter()
            .find(|session| session.session_id == session_id)
            .and_then(|session| session.profile_id),
        )
    }

    async fn ensure_external_session_thread(
        &self,
        prepared: &PreparedExternalSessionThread,
    ) -> anyhow::Result<WorkspaceThread> {
        let thread = &prepared.thread;
        self.ensure_thread_persisted(thread).await?;
        if thread.status != ThreadStatus::Closed && thread.host_binding != prepared.host_binding {
            self.thread_store
                .append(&ThreadEvent::host_changed(
                    thread.id.clone(),
                    prepared.host_binding.clone(),
                ))
                .await
                .context("append external session host binding")?;
            self.notify_change();
        }
        if thread.status != ThreadStatus::Closed
            && !thread.has_agent_session(&prepared.host_binding, &prepared.session_id)
        {
            self.thread_store
                .append(&ThreadEvent::agent_session_observed(
                    thread.id.clone(),
                    prepared.agent_id.clone(),
                    prepared.profile_id.clone(),
                    prepared.session_id.clone(),
                ))
                .await
                .context("append external session")?;
            self.notify_change();
        }
        self.thread(&thread.id).await?.ok_or_else(|| {
            anyhow!(
                "thread {} not found after external session attach",
                thread.id
            )
        })
    }
}
