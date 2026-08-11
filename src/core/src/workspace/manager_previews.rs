use super::*;

impl WorkspaceThreadManager {
    pub async fn attach_preview_web_thread_hint(
        &self,
        preview_slug: &str,
        thread_id: &WorkspaceThreadId,
    ) -> anyhow::Result<Option<Arc<ThreadRuntime>>> {
        let _preview_lifecycle = self.preview_lifecycle.lock().await;
        if crate::previews::lookup_owner(preview_slug).is_none() {
            return Err(anyhow!("Preview {} no longer exists", preview_slug));
        }
        let route = preview_web_route_for_slug(preview_slug);
        if let Some(active) = self.active_runtime_for_route(&route).await? {
            return Ok(Some(active));
        }
        let Some(thread) = self.thread(thread_id).await? else {
            return Ok(None);
        };
        if thread.status != ThreadStatus::Open
            || thread.preview_slug.as_deref() != Some(preview_slug)
        {
            return Ok(None);
        }

        let already_attached = self
            .current_attachment(&route)
            .await?
            .is_some_and(|attachment| attachment.thread_id == thread.id);
        if !already_attached {
            self.attach_route(route, thread.workspace_id.clone(), thread.id.clone())
                .await?;
        }
        crate::previews::replace_owner_conversation(preview_slug, thread.id.clone())
            .map_err(|_| anyhow!("Preview {} no longer exists", preview_slug))?;
        self.runtime_from_thread(thread).await.map(Some)
    }

    pub async fn ensure_preview_web_thread(
        &self,
        parent_thread_id: Option<&WorkspaceThreadId>,
        preview_slug: &str,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        let _preview_lifecycle = self.preview_lifecycle.lock().await;
        let preview = crate::previews::lookup_owner(preview_slug)
            .ok_or_else(|| anyhow!("Preview {} no longer exists", preview_slug))?;
        let route = preview_web_route_for_slug(preview_slug);
        if let Some(bound_thread_id) = crate::previews::owner_conversation_thread_id(preview_slug) {
            if let Some(bound) = self.thread(&bound_thread_id).await? {
                if bound.preview_slug.as_deref() != Some(preview_slug) {
                    return Err(anyhow!(
                        "Preview {} is linked to unrelated task {}",
                        preview_slug,
                        bound_thread_id
                    ));
                }
                return self
                    .reuse_or_advance_preview_thread(&route, preview_slug, bound)
                    .await;
            }
        }

        if let Some(previous) = self.preferred_preview_thread(preview_slug).await? {
            return self
                .reuse_or_advance_preview_thread(&route, preview_slug, previous)
                .await;
        }

        let (workspace_id, parent_thread_id, host_binding) = match parent_thread_id {
            Some(parent_thread_id) => {
                let parent = self
                    .thread(parent_thread_id)
                    .await?
                    .ok_or_else(|| anyhow!("thread {} not found", parent_thread_id))?;
                (parent.workspace_id, Some(parent.id), parent.host_binding)
            }
            None => {
                let workspace = self.ensure_workspace_for_cwd(preview.workspace).await?;
                let host_binding = default_route_binding_and_workspace(&route).0;
                (workspace.id, None, host_binding)
            }
        };
        self.create_preview_thread_for_route(
            &route,
            workspace_id,
            parent_thread_id,
            preview_slug.to_string(),
            host_binding,
        )
        .await
    }

    pub(super) async fn create_preview_thread_for_route(
        &self,
        route: &RouteKey,
        workspace_id: WorkspaceId,
        parent_thread_id: Option<WorkspaceThreadId>,
        preview_slug: String,
        host_binding: HostBinding,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        let event = ThreadEvent::preview_created(
            WorkspaceThreadId::new(),
            workspace_id,
            parent_thread_id,
            preview_slug.clone(),
            host_binding,
        );
        let task = ThreadProjection::from_events(&[event])?
            .all()
            .next()
            .cloned()
            .expect("created Preview task");
        self.ensure_thread_persisted(&task).await?;
        self.attach_route(route.clone(), task.workspace_id.clone(), task.id.clone())
            .await?;
        crate::previews::replace_owner_conversation(&preview_slug, task.id.clone())
            .map_err(|_| anyhow!("Preview {} no longer exists", preview_slug))?;
        self.runtime_from_thread(task).await
    }

    pub(super) async fn resolve_preview_route_runtime_locked(
        &self,
        route: &RouteKey,
        preview_slug: &str,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        if crate::previews::lookup_owner(preview_slug).is_none() {
            return Err(anyhow!("Preview {} no longer exists", preview_slug));
        }
        if let Some(thread_id) = crate::previews::owner_conversation_thread_id(preview_slug) {
            if let Some(thread) = self.thread(&thread_id).await? {
                if thread.preview_slug.as_deref() == Some(preview_slug) {
                    return self
                        .reuse_or_advance_preview_thread(route, preview_slug, thread)
                        .await;
                }
            }
        }

        let previous = self
            .preferred_preview_thread(preview_slug)
            .await?
            .ok_or_else(|| anyhow!("Preview {} conversation is not initialized", preview_slug))?;
        self.reuse_or_advance_preview_thread(route, preview_slug, previous)
            .await
    }

    async fn preferred_preview_thread(
        &self,
        preview_slug: &str,
    ) -> anyhow::Result<Option<WorkspaceThread>> {
        let projection = self.thread_projection().await?;
        let mut latest_open = None;
        let mut latest_any = None;
        for thread in projection
            .all()
            .filter(|thread| thread.preview_slug.as_deref() == Some(preview_slug))
        {
            if latest_any
                .is_none_or(|latest: &WorkspaceThread| latest.updated_at < thread.updated_at)
            {
                latest_any = Some(thread);
            }
            if thread.status == ThreadStatus::Open
                && latest_open
                    .is_none_or(|latest: &WorkspaceThread| latest.updated_at < thread.updated_at)
            {
                latest_open = Some(thread);
            }
        }
        Ok(latest_open.or(latest_any).cloned())
    }

    async fn reuse_or_advance_preview_thread(
        &self,
        route: &RouteKey,
        preview_slug: &str,
        previous: WorkspaceThread,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        if previous.status == ThreadStatus::Open {
            self.attach_route(
                route.clone(),
                previous.workspace_id.clone(),
                previous.id.clone(),
            )
            .await?;
            crate::previews::replace_owner_conversation(preview_slug, previous.id.clone())
                .map_err(|_| anyhow!("Preview {} no longer exists", preview_slug))?;
            return self.runtime_from_thread(previous).await;
        }

        self.create_preview_thread_for_route(
            route,
            previous.workspace_id,
            previous.parent_thread_id,
            preview_slug.to_string(),
            previous.host_binding,
        )
        .await
    }
}
