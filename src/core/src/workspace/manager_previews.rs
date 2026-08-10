use super::*;

impl WorkspaceThreadManager {
    pub async fn ensure_preview_child_web_thread(
        &self,
        parent_thread_id: &WorkspaceThreadId,
        preview_slug: &str,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        let parent = self
            .thread(parent_thread_id)
            .await?
            .ok_or_else(|| anyhow!("thread {} not found", parent_thread_id))?;
        if let Some(bound_thread_id) = crate::previews::owner_conversation_thread_id(preview_slug) {
            let bound = self
                .thread(&bound_thread_id)
                .await?
                .ok_or_else(|| anyhow!("thread {} not found", bound_thread_id))?;
            if bound.parent_thread_id.as_ref() != Some(parent_thread_id)
                || bound.preview_slug.as_deref() != Some(preview_slug)
            {
                return Err(anyhow!(
                    "Preview {} is already linked to task {}",
                    preview_slug,
                    bound_thread_id
                ));
            }
            let route = preview_web_route_for_slug(preview_slug);
            if bound.status == ThreadStatus::Open {
                self.attach_route(route, bound.workspace_id.clone(), bound.id.clone())
                    .await?;
                return self.runtime_from_thread(bound).await;
            }
            return self
                .create_preview_child_for_route(
                    &route,
                    bound.workspace_id,
                    parent_thread_id.clone(),
                    preview_slug.to_string(),
                    bound.host_binding,
                )
                .await;
        }

        let existing = self
            .thread_projection()
            .await?
            .all()
            .filter(|thread| thread.status == ThreadStatus::Open)
            .filter(|thread| thread.parent_thread_id.as_ref() == Some(parent_thread_id))
            .filter(|thread| thread.preview_slug.as_deref() == Some(preview_slug))
            .max_by(|left, right| left.updated_at.cmp(&right.updated_at))
            .cloned();
        let runtime = match existing {
            Some(thread) => {
                let route = preview_web_route_for_slug(preview_slug);
                self.attach_route(route, thread.workspace_id.clone(), thread.id.clone())
                    .await?;
                self.runtime_from_thread(thread).await?
            }
            None => {
                self.create_preview_child_for_route(
                    &preview_web_route_for_slug(preview_slug),
                    parent.workspace_id,
                    parent.id,
                    preview_slug.to_string(),
                    parent.host_binding,
                )
                .await?
            }
        };
        let thread_id = runtime.state().await.thread_id;
        crate::previews::bind_owner_conversation(preview_slug, thread_id).map_err(|error| {
            anyhow!(
                "failed to bind Preview {} conversation: {:?}",
                preview_slug,
                error
            )
        })?;
        Ok(runtime)
    }

    pub(super) async fn create_preview_child_for_route(
        &self,
        route: &RouteKey,
        workspace_id: WorkspaceId,
        parent_thread_id: WorkspaceThreadId,
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
        let child = ThreadProjection::from_events(&[event])?
            .all()
            .next()
            .cloned()
            .expect("created Preview child");
        self.ensure_thread_persisted(&child).await?;
        self.attach_route(route.clone(), child.workspace_id.clone(), child.id.clone())
            .await?;
        crate::previews::replace_owner_conversation(&preview_slug, child.id.clone())
            .map_err(|_| anyhow!("Preview {} no longer exists", preview_slug))?;
        self.runtime_from_thread(child).await
    }

    pub(super) async fn resolve_preview_route_runtime(
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
                    if thread.status == ThreadStatus::Closed {
                        let parent_thread_id = thread.parent_thread_id.ok_or_else(|| {
                            anyhow!(
                                "Preview {} history is missing its parent task",
                                preview_slug
                            )
                        })?;
                        return self
                            .create_preview_child_for_route(
                                route,
                                thread.workspace_id,
                                parent_thread_id,
                                preview_slug.to_string(),
                                thread.host_binding,
                            )
                            .await;
                    }
                    self.attach_route(
                        route.clone(),
                        thread.workspace_id.clone(),
                        thread.id.clone(),
                    )
                    .await?;
                    return self.runtime_from_thread(thread).await;
                }
            }
        }

        let previous = self
            .thread_projection()
            .await?
            .all()
            .filter(|thread| thread.preview_slug.as_deref() == Some(preview_slug))
            .max_by(|left, right| left.updated_at.cmp(&right.updated_at))
            .cloned()
            .ok_or_else(|| anyhow!("Preview {} conversation is not initialized", preview_slug))?;

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

        let parent_thread_id = previous.parent_thread_id.ok_or_else(|| {
            anyhow!(
                "Preview {} history is missing its parent task",
                preview_slug
            )
        })?;
        self.create_preview_child_for_route(
            route,
            previous.workspace_id,
            parent_thread_id,
            preview_slug.to_string(),
            previous.host_binding,
        )
        .await
    }
}
