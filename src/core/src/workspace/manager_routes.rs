use super::*;

impl WorkspaceThreadManager {
    pub async fn switch_workspace_id(
        &self,
        route: &RouteKey,
        workspace_id: &str,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        let workspace_id = WorkspaceId::from(workspace_id.trim());
        let workspace = self
            .workspace(&workspace_id)
            .await?
            .filter(|workspace| !workspace.archived)
            .ok_or_else(|| anyhow!("workspace '{}' not found", workspace_id))?;
        self.create_thread_for_route(route, workspace.id).await
    }

    pub async fn initialize_multi_agent_turn(
        &self,
        thread_id: &WorkspaceThreadId,
        turn: MultiAgentTurn,
        agents: Vec<ThreadAgent>,
    ) -> anyhow::Result<()> {
        let runtime = self.runtime_for_thread(thread_id).await?;
        runtime
            .initialize_multi_agent_turn(turn, agents)
            .await
            .map_err(|error| anyhow!(error.to_string()))
    }

    pub async fn runtime_for_thread_id(
        &self,
        thread_id: &WorkspaceThreadId,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        self.runtime_for_thread(thread_id).await
    }

    pub async fn active_route_runtime(
        &self,
        route: &RouteKey,
    ) -> anyhow::Result<Option<Arc<ThreadRuntime>>> {
        self.active_runtime_for_route(route).await
    }

    pub async fn cancel_route(&self, route: &RouteKey) -> anyhow::Result<bool> {
        let Some(runtime) = self.active_runtime_for_route(route).await? else {
            return Ok(false);
        };
        match runtime.cancel().await {
            Ok(()) => Ok(true),
            Err(error) => {
                tracing::debug!(
                    route = %route,
                    error = %error.message,
                    "ignored route cancel"
                );
                Ok(false)
            }
        }
    }

    pub async fn attach_thread(
        &self,
        route: &RouteKey,
        thread_id: &WorkspaceThreadId,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        let thread = self
            .thread(thread_id)
            .await?
            .ok_or_else(|| anyhow!("thread {} not found", thread_id))?;
        self.attach_route(
            route.clone(),
            thread.workspace_id.clone(),
            thread.id.clone(),
        )
        .await?;
        self.runtime_from_thread(thread).await
    }

    pub async fn current_attachment(
        &self,
        route: &RouteKey,
    ) -> anyhow::Result<Option<crate::workspace::threads::attachment::RouteAttachment>> {
        Ok(self.attachment_projection().await?.get(route).cloned())
    }

    pub async fn attached_routes_for_thread(
        &self,
        thread_id: &WorkspaceThreadId,
    ) -> anyhow::Result<Vec<RouteKey>> {
        Ok(self
            .attachment_projection()
            .await?
            .all()
            .filter(|attachment| &attachment.thread_id == thread_id)
            .filter(|attachment| attachment.visibility == RouteAttachmentVisibility::Visible)
            .map(|attachment| attachment.route.clone())
            .collect())
    }

    pub async fn reset_thread_attachments_for_host_start(
        &self,
        thread_id: &WorkspaceThreadId,
        current_route: Option<&RouteKey>,
    ) -> anyhow::Result<()> {
        let Some(route) = current_route else {
            return Ok(());
        };
        let projection = self.attachment_projection().await?;
        if let Some(attachment) = projection.get(route) {
            if attachment.visibility == RouteAttachmentVisibility::Hidden {
                return Ok(());
            }
            if &attachment.thread_id == thread_id {
                return Ok(());
            }
        }
        let thread = self
            .thread(thread_id)
            .await?
            .ok_or_else(|| anyhow!("thread {} not found", thread_id))?;
        self.attachment_store
            .append(&RouteAttachmentEvent::attached(
                route.clone(),
                thread.workspace_id,
                thread.id,
            ))
            .await
            .context("append route attach for host start")?;

        self.notify_change();
        Ok(())
    }

    pub async fn runtime_entries(&self) -> anyhow::Result<Vec<WorkspaceThreadRuntimeEntry>> {
        let thread_projection = self.thread_projection().await?;
        let attachment_projection = self.attachment_projection().await?;
        let mut routes_by_thread: HashMap<WorkspaceThreadId, Vec<_>> = HashMap::new();
        for attachment in attachment_projection.all() {
            routes_by_thread
                .entry(attachment.thread_id.clone())
                .or_default()
                .push(attachment.clone());
        }

        let mut entries = Vec::new();
        let runtimes = self.runtimes.entries().await;
        for (thread_id, runtime) in runtimes {
            let Some(thread) = thread_projection.get(&thread_id) else {
                continue;
            };
            if thread.status != ThreadStatus::Open {
                continue;
            }
            let state = runtime.state().await;
            if !runtime_has_started_host(&state) {
                continue;
            }
            let mut attached_routes = routes_by_thread
                .get(&thread.id)
                .cloned()
                .unwrap_or_default();
            attached_routes.sort_by_key(|attachment| attachment.route.display_key());
            let visible_routes = attached_routes
                .iter()
                .filter(|attachment| attachment.visibility == RouteAttachmentVisibility::Visible)
                .map(|attachment| attachment.route.clone())
                .collect::<Vec<_>>();
            entries.push(WorkspaceThreadRuntimeEntry {
                route: visible_routes.first().cloned().or_else(|| {
                    attached_routes
                        .first()
                        .map(|attachment| attachment.route.clone())
                }),
                attached_routes: visible_routes,
                first_user_prompt: thread.first_user_prompt.clone(),
                created_at: thread.created_at.clone(),
                updated_at: thread.updated_at.clone(),
                state,
            });
        }
        entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(entries)
    }

    pub(crate) async fn reconcile_warm_thread_pool(
        self: &Arc<Self>,
        protected_thread_id: &WorkspaceThreadId,
    ) {
        let now = tokio::time::Instant::now();
        let entries = self.runtimes.entries().await;
        let resident_count = entries
            .iter()
            .filter(|(_, runtime)| runtime.thread_activity().live)
            .count();
        if resident_count <= MAX_WARM_THREADS {
            return;
        }

        let candidate = oldest_evictable_warm_thread(
            entries.into_iter().map(|(thread_id, runtime)| {
                let activity = runtime.thread_activity();
                (thread_id, runtime, activity)
            }),
            protected_thread_id,
            now,
        );

        let Some((thread_id, runtime, activity)) = candidate else {
            tracing::debug!(
                resident_count,
                max_warm_threads = MAX_WARM_THREADS,
                "warm thread pool temporarily exceeds its soft limit"
            );
            return;
        };

        if runtime.evict_if_idle(activity.generation).await {
            tracing::info!(
                thread_id = %thread_id,
                resident_count,
                max_warm_threads = MAX_WARM_THREADS,
                "evicted least-recently-used warm thread"
            );
        }
    }

    pub async fn shutdown_all(&self) {
        let runtimes = self.runtimes.values().await;
        for runtime in runtimes {
            runtime.shutdown_host().await;
        }
    }

    pub(super) async fn active_runtime_for_route(
        &self,
        route: &RouteKey,
    ) -> anyhow::Result<Option<Arc<ThreadRuntime>>> {
        let Some(attached) = self.current_attachment(route).await? else {
            return Ok(None);
        };
        if let Some(runtime) = self.runtimes.get(&attached.thread_id).await {
            return Ok(Some(runtime));
        }

        let Some(thread) = self.thread(&attached.thread_id).await? else {
            self.detach_route(route).await?;
            return Ok(None);
        };
        if thread.status != ThreadStatus::Open {
            self.runtimes.remove(attached.thread_id.clone());
            self.detach_route(route).await?;
            return Ok(None);
        }
        if route_can_rehydrate_runtime(route) {
            return self.runtime_from_thread(thread).await.map(Some);
        }
        self.detach_route(route).await?;
        Ok(None)
    }

    pub(super) async fn attach_route(
        &self,
        route: RouteKey,
        workspace_id: WorkspaceId,
        thread_id: WorkspaceThreadId,
    ) -> anyhow::Result<()> {
        self.attachment_store
            .append(&RouteAttachmentEvent::attached(
                route,
                workspace_id,
                thread_id,
            ))
            .await
            .context("append route attachment")?;
        self.notify_change();
        Ok(())
    }
}

fn oldest_evictable_warm_thread<T>(
    candidates: impl IntoIterator<
        Item = (
            WorkspaceThreadId,
            T,
            crate::workspace::threads::runtime::ThreadActivitySnapshot,
        ),
    >,
    protected_thread_id: &WorkspaceThreadId,
    now: tokio::time::Instant,
) -> Option<(
    WorkspaceThreadId,
    T,
    crate::workspace::threads::runtime::ThreadActivitySnapshot,
)> {
    candidates
        .into_iter()
        .filter(|(thread_id, _, activity)| {
            thread_id != protected_thread_id
                && activity.live
                && !activity.busy
                && !activity.has_subagents
                && now.duration_since(activity.last_activity_at) >= WARM_THREAD_MIN_IDLE
        })
        .min_by_key(|(_, _, activity)| activity.last_activity_at)
}

#[cfg(test)]
mod warm_thread_tests {
    use super::*;
    use crate::workspace::threads::runtime::ThreadActivitySnapshot;

    fn activity(
        now: tokio::time::Instant,
        idle_for: Duration,
        live: bool,
        busy: bool,
        has_subagents: bool,
    ) -> ThreadActivitySnapshot {
        ThreadActivitySnapshot {
            live,
            busy,
            has_subagents,
            generation: 1,
            last_activity_at: now - idle_for,
        }
    }

    #[test]
    fn warm_thread_eviction_selects_the_oldest_eligible_thread() {
        let now = tokio::time::Instant::now();
        let protected = WorkspaceThreadId::from("new");
        let candidates = vec![
            (
                WorkspaceThreadId::from("new"),
                (),
                activity(now, Duration::from_secs(30 * 60), true, false, false),
            ),
            (
                WorkspaceThreadId::from("recent"),
                (),
                activity(now, Duration::from_secs(9 * 60), true, false, false),
            ),
            (
                WorkspaceThreadId::from("old"),
                (),
                activity(now, Duration::from_secs(20 * 60), true, false, false),
            ),
            (
                WorkspaceThreadId::from("oldest"),
                (),
                activity(now, Duration::from_secs(30 * 60), true, false, false),
            ),
        ];

        let selected = oldest_evictable_warm_thread(candidates, &protected, now)
            .expect("an old idle host should be eligible");

        assert_eq!(selected.0, WorkspaceThreadId::from("oldest"));
    }

    #[test]
    fn warm_thread_eviction_excludes_unsafe_candidates() {
        let now = tokio::time::Instant::now();
        let protected = WorkspaceThreadId::from("new");
        let old = Duration::from_secs(20 * 60);
        let candidates = vec![
            (
                WorkspaceThreadId::from("busy"),
                (),
                activity(now, old, true, true, false),
            ),
            (
                WorkspaceThreadId::from("subagents"),
                (),
                activity(now, old, true, false, true),
            ),
            (
                WorkspaceThreadId::from("stopped"),
                (),
                activity(now, old, false, false, false),
            ),
            (
                WorkspaceThreadId::from("recent"),
                (),
                activity(now, Duration::from_secs(9 * 60), true, false, false),
            ),
        ];

        assert!(oldest_evictable_warm_thread(candidates, &protected, now).is_none());
    }

    #[test]
    fn warm_thread_is_eligible_at_the_idle_threshold() {
        let now = tokio::time::Instant::now();
        let protected = WorkspaceThreadId::from("new");
        let candidates = vec![(
            WorkspaceThreadId::from("exactly-ten-minutes"),
            (),
            activity(now, WARM_THREAD_MIN_IDLE, true, false, false),
        )];

        assert!(oldest_evictable_warm_thread(candidates, &protected, now).is_some());
    }
}
