//! Workspace/thread orchestration.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context};
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use tokio::sync::{broadcast, Mutex};

use crate::agent::launch::normalize_launch_profile_id;
use crate::agent_state;
use crate::routing::{channel_traits, DefaultWorkspaceKind, RouteKey};

use super::registry::{WorkspaceId, WorkspaceProjection, WorkspaceRecord, GENERAL_WORKSPACE_ID};
use super::store::{WorkspaceEvent, WorkspaceEventStore};
use super::threads::attachment::{
    RouteAttachmentEvent, RouteAttachmentEventStore, RouteAttachmentProjection,
};
use super::threads::runtime::ThreadRuntime;
use super::threads::runtime::ThreadRuntimeState;
use super::threads::store::{
    HostBinding, MultiAgentTurn, ThreadAgent, ThreadEvent, ThreadEventStore, ThreadProjection,
    ThreadStatus, WorkspaceThread, WorkspaceThreadId,
};

pub const AGENT_HOST_IDLE_SHUTDOWN_DELAY: Duration = Duration::from_secs(10 * 60);
const LEGACY_CHANNEL_DEFAULT_CHAT_ID: &str = "__channel_default__";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalSessionAttachMode {
    ReuseOpenThread,
    NewThread,
}

struct PreparedExternalSessionThread {
    workspace: WorkspaceRecord,
    host_binding: HostBinding,
    agent_id: String,
    profile_id: Option<String>,
    session_id: String,
    thread: WorkspaceThread,
}

pub fn web_chat_id_for_thread(thread_id: &WorkspaceThreadId) -> String {
    format!("ws_{}", thread_id.as_str())
}

pub fn web_route_for_thread(thread_id: &WorkspaceThreadId) -> RouteKey {
    RouteKey::new("web", web_chat_id_for_thread(thread_id))
}

pub struct WorkspaceThreadManager {
    workspace_store: WorkspaceEventStore,
    thread_store: ThreadEventStore,
    attachment_store: RouteAttachmentEventStore,
    runtimes: DashMap<WorkspaceThreadId, Arc<ThreadRuntime>>,
    route_locks: DashMap<RouteKey, Arc<Mutex<()>>>,
    change_tx: broadcast::Sender<()>,
}

impl WorkspaceThreadManager {
    pub fn new_default() -> Arc<Self> {
        let (change_tx, _) = broadcast::channel(64);
        Arc::new(Self {
            workspace_store: WorkspaceEventStore::new(WorkspaceEventStore::default_path()),
            thread_store: ThreadEventStore::new(ThreadEventStore::default_path()),
            attachment_store: RouteAttachmentEventStore::new(
                RouteAttachmentEventStore::default_path(),
            ),
            runtimes: DashMap::new(),
            route_locks: DashMap::new(),
            change_tx,
        })
    }

    pub fn with_paths(
        workspace_path: PathBuf,
        thread_path: PathBuf,
        attachment_path: PathBuf,
    ) -> Arc<Self> {
        let (change_tx, _) = broadcast::channel(64);
        Arc::new(Self {
            workspace_store: WorkspaceEventStore::new(workspace_path),
            thread_store: ThreadEventStore::new(thread_path),
            attachment_store: RouteAttachmentEventStore::new(attachment_path),
            runtimes: DashMap::new(),
            route_locks: DashMap::new(),
            change_tx,
        })
    }

    pub async fn resolve_route_runtime(
        &self,
        route: &RouteKey,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        let route_lock = self.route_lock(route);
        let _route_guard = route_lock.lock().await;

        if let Some(runtime) = self.active_runtime_for_route(route).await? {
            return Ok(runtime);
        }

        let (host_binding, workspace_path) = default_route_binding_and_workspace(route);
        let workspace = self
            .ensure_default_workspace_for_route(route, workspace_path)
            .await?;
        let thread = self.new_thread_record_with_host(workspace.id.clone(), host_binding);
        self.ensure_thread_persisted(&thread).await?;
        self.attach_route(route.clone(), workspace.id, thread.id.clone())
            .await?;
        self.runtime_from_thread(thread).await
    }

    fn route_lock(&self, route: &RouteKey) -> Arc<Mutex<()>> {
        self.route_locks
            .entry(route.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    pub async fn create_thread_for_route(
        &self,
        route: &RouteKey,
        workspace_id: WorkspaceId,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        let host_binding = match self.active_runtime_for_route(route).await? {
            Some(runtime) => runtime.state().await.host_binding,
            None => default_route_binding_and_workspace(route).0,
        };
        self.create_thread_for_route_with_host(route, workspace_id, host_binding)
            .await
    }

    pub async fn create_thread_for_route_with_host(
        &self,
        route: &RouteKey,
        workspace_id: WorkspaceId,
        host_binding: HostBinding,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        let workspace = self
            .workspace(&workspace_id)
            .await?
            .ok_or_else(|| anyhow!("workspace {} not found", workspace_id))?;
        let thread = self.new_thread_record_with_host(workspace.id.clone(), host_binding);
        self.ensure_thread_persisted(&thread).await?;
        self.attach_route(route.clone(), workspace.id, thread.id.clone())
            .await?;
        self.runtime_from_thread(thread).await
    }

    pub async fn create_thread_in_current_workspace(
        &self,
        route: &RouteKey,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        if let Some(runtime) = self.active_runtime_for_route(route).await? {
            let state = runtime.state().await;
            return self
                .create_thread_for_route_with_host(route, state.workspace_id, state.host_binding)
                .await;
        }

        let (host_binding, workspace_path) = default_route_binding_and_workspace(route);
        let workspace = self
            .ensure_default_workspace_for_route(route, workspace_path)
            .await?;
        self.create_thread_for_route_with_host(route, workspace.id, host_binding)
            .await
    }

    pub async fn create_thread_in_current_workspace_with_host(
        &self,
        route: &RouteKey,
        host_binding: HostBinding,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        if let Some(runtime) = self.active_runtime_for_route(route).await? {
            let state = runtime.state().await;
            return self
                .create_thread_for_route_with_host(route, state.workspace_id, host_binding)
                .await;
        }

        let (_, workspace_path) = default_route_binding_and_workspace(route);
        let workspace = self
            .ensure_default_workspace_for_route(route, workspace_path)
            .await?;
        self.create_thread_for_route_with_host(route, workspace.id, host_binding)
            .await
    }

    pub async fn close_route_and_create_thread(
        &self,
        route: &RouteKey,
        reason: Option<String>,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        let current = match self.active_runtime_for_route(route).await? {
            Some(runtime) => {
                let state = runtime.state().await;
                runtime
                    .close(reason)
                    .await
                    .map_err(|error| anyhow!(error.to_string()))?;
                self.runtimes.remove(&state.thread_id);
                self.detach_route(route).await?;
                Some((state.workspace_id, state.host_binding))
            }
            None => None,
        };

        if let Some((workspace_id, host_binding)) = current {
            self.create_thread_for_route_with_host(route, workspace_id, host_binding)
                .await
        } else {
            self.create_thread_in_current_workspace(route).await
        }
    }

    pub async fn create_thread_for_cwd(
        &self,
        route: &RouteKey,
        cwd: PathBuf,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        let workspace = self.ensure_workspace_for_cwd(cwd).await?;
        self.create_thread_for_route(route, workspace.id).await
    }

    pub async fn close_route(
        &self,
        route: &RouteKey,
        reason: Option<String>,
    ) -> anyhow::Result<()> {
        let Some(runtime) = self.active_runtime_for_route(route).await? else {
            return Ok(());
        };
        let thread_id = runtime.state().await.thread_id;
        runtime
            .close(reason)
            .await
            .map_err(|error| anyhow!(error.to_string()))?;
        self.runtimes.remove(&thread_id);
        self.detach_route(route).await
    }

    pub async fn detach_route(&self, route: &RouteKey) -> anyhow::Result<()> {
        self.attachment_store
            .append(&RouteAttachmentEvent::detached(route.clone()))
            .await
            .context("append route detach")?;
        self.attachment_store
            .compact()
            .await
            .context("compact route attachments")?;
        self.notify_change();
        Ok(())
    }

    pub async fn close_thread(
        &self,
        thread_id: &WorkspaceThreadId,
        reason: Option<String>,
    ) -> anyhow::Result<()> {
        let runtime = self.runtime_for_thread(thread_id).await?;
        runtime
            .close(reason)
            .await
            .map_err(|error| anyhow!(error.to_string()))?;
        self.runtimes.remove(thread_id);
        self.notify_change();
        Ok(())
    }

    pub async fn shutdown_route_host(&self, route: &RouteKey) -> anyhow::Result<()> {
        let Some(runtime) = self.active_runtime_for_route(route).await? else {
            return Ok(());
        };
        self.shutdown_thread_host(&runtime.state().await.thread_id)
            .await
    }

    pub async fn shutdown_thread_host(&self, thread_id: &WorkspaceThreadId) -> anyhow::Result<()> {
        let Some(runtime) = self
            .runtimes
            .get(thread_id)
            .map(|entry| Arc::clone(entry.value()))
        else {
            return Ok(());
        };
        runtime.shutdown_host().await;
        self.runtimes.remove(thread_id);
        self.notify_change();
        Ok(())
    }

    pub async fn attach_external_session(
        &self,
        route: &RouteKey,
        agent_id: String,
        profile_id: Option<String>,
        session_id: String,
        cwd: PathBuf,
        mode: ExternalSessionAttachMode,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        let prepared = self
            .prepare_external_session_thread(agent_id, profile_id, session_id, cwd, mode)
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
        mode: ExternalSessionAttachMode,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        let prepared = self
            .prepare_external_session_thread(agent_id, profile_id, session_id, cwd, mode)
            .await?;
        let route = web_route_for_thread(&prepared.thread.id);
        let thread = self.ensure_external_session_thread(&prepared).await?;
        self.attach_route(route, prepared.workspace.id.clone(), thread.id.clone())
            .await?;
        self.runtime_from_thread(thread).await
    }

    pub async fn create_web_thread_for_cwd_with_host(
        &self,
        agent_id: String,
        profile_id: Option<String>,
        cwd: PathBuf,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        let profile_id = normalize_optional_launch_profile_id(profile_id.as_deref())
            .or_else(|| launch_setting_profile_for_agent(&agent_id));
        let workspace = self.ensure_workspace_for_cwd(cwd).await?;
        let host_binding = HostBinding::new(agent_id, profile_id);
        let thread = self.new_thread_record_with_host(workspace.id.clone(), host_binding);
        let route = web_route_for_thread(&thread.id);
        self.ensure_thread_persisted(&thread).await?;
        self.attach_route(route, workspace.id, thread.id.clone())
            .await?;
        self.runtime_from_thread(thread).await
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
        mode: ExternalSessionAttachMode,
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
        let thread = if mode == ExternalSessionAttachMode::ReuseOpenThread {
            // TODO: Revisit persisted thread lifecycle states here. "Closed" can
            // mean user-ended, while Web idle only unloads the runtime; session
            // resume should not accidentally split one native session across
            // multiple workspace thread ids.
            projection
                .for_workspace(&workspace.id, false)
                .find(|thread| {
                    thread.status != ThreadStatus::Closed
                        && thread.agent_sessions.values().flatten().any(|session| {
                            session.agent_id == agent_id && session.session_id == session_id
                        })
                })
                .cloned()
        } else {
            None
        };
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
            self.new_thread_record_with_host(workspace.id.clone(), host_binding.clone())
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
    ) -> anyhow::Result<Option<super::threads::attachment::RouteAttachment>> {
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
            .filter(|attachment| !is_legacy_channel_default_route(&attachment.route))
            .map(|attachment| attachment.route.clone())
            .collect())
    }

    pub async fn reset_thread_attachments_for_host_start(
        &self,
        thread_id: &WorkspaceThreadId,
        current_route: Option<&RouteKey>,
    ) -> anyhow::Result<()> {
        let Some(route) = current_route.filter(|route| !is_legacy_channel_default_route(route))
        else {
            return Ok(());
        };
        let projection = self.attachment_projection().await?;
        if projection
            .get(route)
            .is_some_and(|attachment| &attachment.thread_id == thread_id)
        {
            return Ok(());
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

        self.attachment_store
            .compact()
            .await
            .context("compact route attachments after host start")?;
        self.notify_change();
        Ok(())
    }

    pub async fn runtime_entries(&self) -> anyhow::Result<Vec<WorkspaceThreadRuntimeEntry>> {
        let thread_projection = self.thread_projection().await?;
        let attachment_projection = self.attachment_projection().await?;
        let mut routes_by_thread: HashMap<WorkspaceThreadId, Vec<RouteKey>> = HashMap::new();
        for attachment in attachment_projection.all() {
            routes_by_thread
                .entry(attachment.thread_id.clone())
                .or_default()
                .push(attachment.route.clone());
        }

        let mut entries = Vec::new();
        let runtimes: Vec<(WorkspaceThreadId, Arc<ThreadRuntime>)> = self
            .runtimes
            .iter()
            .map(|entry| (entry.key().clone(), Arc::clone(entry.value())))
            .collect();
        for (thread_id, runtime) in runtimes {
            let Some(thread) = thread_projection.get(&thread_id) else {
                continue;
            };
            if thread.status != super::threads::store::ThreadStatus::Open {
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
            attached_routes.sort_by_key(|route| route.as_key());
            let visible_routes = attached_routes
                .iter()
                .filter(|route| !is_legacy_channel_default_route(route))
                .cloned()
                .collect::<Vec<_>>();
            entries.push(WorkspaceThreadRuntimeEntry {
                route: visible_routes
                    .first()
                    .cloned()
                    .or_else(|| attached_routes.first().cloned()),
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

    pub async fn schedule_route_host_idle_shutdown(
        self: &Arc<Self>,
        route: &RouteKey,
    ) -> anyhow::Result<()> {
        if let Some(runtime) = self.active_runtime_for_route(route).await? {
            self.schedule_host_idle_shutdown(runtime.state().await.thread_id);
        }
        Ok(())
    }

    pub fn schedule_host_idle_shutdown(self: &Arc<Self>, thread_id: WorkspaceThreadId) {
        self.schedule_host_idle_shutdown_after(thread_id, AGENT_HOST_IDLE_SHUTDOWN_DELAY);
    }

    pub fn schedule_host_idle_shutdown_after(
        self: &Arc<Self>,
        thread_id: WorkspaceThreadId,
        delay: Duration,
    ) {
        let Some(runtime) = self
            .runtimes
            .get(&thread_id)
            .map(|entry| Arc::clone(entry.value()))
        else {
            return;
        };
        let generation = runtime.idle_generation();
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            if !runtime.shutdown_host_if_idle(generation).await {
                return;
            }
            let should_remove = manager
                .runtimes
                .get(&thread_id)
                .map(|entry| Arc::ptr_eq(entry.value(), &runtime))
                .unwrap_or(false);
            if should_remove {
                manager.runtimes.remove(&thread_id);
            }
            manager.notify_change();
        });
    }

    pub async fn shutdown_all(&self) {
        let runtimes: Vec<Arc<ThreadRuntime>> = self
            .runtimes
            .iter()
            .map(|entry| Arc::clone(entry.value()))
            .collect();
        for runtime in runtimes {
            runtime.shutdown_host().await;
        }
    }

    async fn active_runtime_for_route(
        &self,
        route: &RouteKey,
    ) -> anyhow::Result<Option<Arc<ThreadRuntime>>> {
        let Some(attached) = self.current_attachment(route).await? else {
            return Ok(None);
        };
        if let Some(runtime) = self
            .runtimes
            .get(&attached.thread_id)
            .map(|entry| Arc::clone(entry.value()))
        {
            return Ok(Some(runtime));
        }

        let Some(thread) = self.thread(&attached.thread_id).await? else {
            self.detach_route(route).await?;
            return Ok(None);
        };
        if thread.status != ThreadStatus::Open {
            self.runtimes.remove(&attached.thread_id);
            self.detach_route(route).await?;
            return Ok(None);
        }
        if route_can_rehydrate_runtime(route) {
            return self.runtime_from_thread(thread).await.map(Some);
        }
        self.detach_route(route).await?;
        Ok(None)
    }

    async fn attach_route(
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

    async fn ensure_general_workspace(&self) -> anyhow::Result<WorkspaceRecord> {
        let projection = self.workspace_projection().await?;
        if let Some(workspace) = projection.get(&WorkspaceId::general()) {
            return Ok(workspace.clone());
        }

        let cwd = normalize_workspace_cwd(crate::config::ensure_loaded().resolve_workspace(""));
        if let Some(workspace) = workspace_by_cwd(&projection, &cwd) {
            return Ok(workspace.clone());
        }

        let event = WorkspaceEvent::registered(WorkspaceId::general(), cwd, "General", true);
        self.workspace_store
            .append(&event)
            .await
            .context("append general workspace")?;
        self.notify_change();
        Ok(WorkspaceProjection::from_events(&[event])?
            .get(&WorkspaceId::general())
            .cloned()
            .expect("registered general workspace"))
    }

    async fn ensure_default_workspace_for_route(
        &self,
        route: &RouteKey,
        workspace_path: PathBuf,
    ) -> anyhow::Result<WorkspaceRecord> {
        match channel_traits(&route.channel_kind).default_workspace {
            DefaultWorkspaceKind::General => self.ensure_general_workspace().await,
            DefaultWorkspaceKind::ChannelDefault => {
                self.ensure_workspace_for_cwd(workspace_path).await
            }
        }
    }

    async fn ensure_workspace_for_cwd(&self, cwd: PathBuf) -> anyhow::Result<WorkspaceRecord> {
        let cwd = normalize_workspace_cwd(cwd);
        let projection = self.workspace_projection().await?;
        if let Some(workspace) = workspace_by_cwd(&projection, &cwd) {
            return Ok(workspace.clone());
        }

        let name = cwd
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("Workspace")
            .to_string();
        let event = WorkspaceEvent::registered(WorkspaceId::new(), cwd, name, false);
        self.workspace_store
            .append(&event)
            .await
            .context("append workspace")?;
        self.notify_change();
        Ok(WorkspaceProjection::from_events(&[event])?
            .all()
            .next()
            .cloned()
            .expect("registered workspace"))
    }

    async fn resolve_workspace(&self, token: &str) -> anyhow::Result<Option<WorkspaceRecord>> {
        let token = token.trim();
        if token.is_empty() {
            return Ok(None);
        }
        let projection = self.workspace_projection().await?;
        if token == GENERAL_WORKSPACE_ID {
            return Ok(projection.get(&WorkspaceId::general()).cloned());
        }
        let id = WorkspaceId::from(token);
        if let Some(workspace) = projection.get(&id) {
            return Ok(Some(workspace.clone()));
        }
        let path = PathBuf::from(token);
        if let Some(workspace) = projection.get_by_cwd(&path) {
            return Ok(Some(workspace.clone()));
        }
        if path.is_dir() {
            let cwd = normalize_workspace_cwd(path);
            if let Some(workspace) = workspace_by_cwd(&projection, &cwd) {
                return Ok(Some(workspace.clone()));
            }
            return self.ensure_workspace_for_cwd(cwd).await.map(Some);
        }
        Ok(None)
    }

    async fn workspace(
        &self,
        workspace_id: &WorkspaceId,
    ) -> anyhow::Result<Option<WorkspaceRecord>> {
        Ok(self
            .workspace_projection()
            .await?
            .get(workspace_id)
            .cloned())
    }

    async fn thread(
        &self,
        thread_id: &WorkspaceThreadId,
    ) -> anyhow::Result<Option<WorkspaceThread>> {
        Ok(self.thread_projection().await?.get(thread_id).cloned())
    }

    async fn ensure_thread_persisted(&self, thread: &WorkspaceThread) -> anyhow::Result<()> {
        if self.thread(&thread.id).await?.is_some() {
            return Ok(());
        }
        self.thread_store
            .append(&ThreadEvent::created(
                thread.id.clone(),
                thread.workspace_id.clone(),
                thread.host_binding.clone(),
            ))
            .await
            .context("append workspace thread")?;
        self.notify_change();
        Ok(())
    }

    fn new_thread_record_with_host(
        &self,
        workspace_id: WorkspaceId,
        host_binding: HostBinding,
    ) -> WorkspaceThread {
        let event = ThreadEvent::created(WorkspaceThreadId::new(), workspace_id, host_binding);
        ThreadProjection::from_events(&[event])
            .expect("single created event should project")
            .all()
            .next()
            .cloned()
            .expect("created thread")
    }

    async fn runtime_for_thread(
        &self,
        thread_id: &WorkspaceThreadId,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        if let Some(runtime) = self.runtimes.get(thread_id) {
            return Ok(Arc::clone(runtime.value()));
        }
        let thread = self
            .thread(thread_id)
            .await?
            .ok_or_else(|| anyhow!("thread {} not found", thread_id))?;
        self.runtime_from_thread(thread).await
    }

    async fn runtime_from_thread(
        &self,
        thread: WorkspaceThread,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        if let Some(runtime) = self.runtimes.get(&thread.id) {
            return Ok(Arc::clone(runtime.value()));
        }
        let workspace = self
            .workspace(&thread.workspace_id)
            .await?
            .ok_or_else(|| anyhow!("workspace {} not found", thread.workspace_id))?;
        let runtime = Arc::new(ThreadRuntime::with_change_tx(
            thread.clone(),
            workspace.cwd,
            self.thread_store.clone(),
            Some(self.change_tx.clone()),
        ));
        match self.runtimes.entry(thread.id.clone()) {
            Entry::Occupied(entry) => return Ok(Arc::clone(entry.get())),
            Entry::Vacant(entry) => {
                entry.insert(Arc::clone(&runtime));
            }
        }
        let recovered = runtime
            .recover_interrupted_subagents()
            .await
            .map_err(|error| anyhow!(error.message.to_string()))?;
        if !recovered.is_empty() {
            tracing::info!(
                thread_id = %thread.id,
                agents = recovered.len(),
                "recovered interrupted subagents"
            );
        }
        self.notify_change();
        Ok(runtime)
    }

    async fn workspace_projection(&self) -> anyhow::Result<WorkspaceProjection> {
        self.workspace_store
            .load_projection()
            .await
            .map_err(|error| anyhow!(error.to_string()))
    }

    async fn thread_projection(&self) -> anyhow::Result<ThreadProjection> {
        self.thread_store
            .load_projection()
            .await
            .map_err(|error| anyhow!(error.to_string()))
    }

    async fn attachment_projection(&self) -> anyhow::Result<RouteAttachmentProjection> {
        self.attachment_store
            .load_projection()
            .await
            .map_err(|error| anyhow!(error.to_string()))
    }

    fn notify_change(&self) {
        let _ = self.change_tx.send(());
    }
}

#[derive(Debug, Clone)]
pub struct WorkspaceThreadRuntimeEntry {
    pub route: Option<RouteKey>,
    pub attached_routes: Vec<RouteKey>,
    pub state: ThreadRuntimeState,
    pub first_user_prompt: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl crate::state::StateSource for WorkspaceThreadManager {
    type Entry = WorkspaceThreadRuntimeEntry;

    async fn list(&self) -> Vec<Self::Entry> {
        match self.runtime_entries().await {
            Ok(entries) => entries,
            Err(error) => {
                tracing::warn!(error = %error, "failed to list workspace thread runtimes");
                Vec::new()
            }
        }
    }

    fn subscribe_changes(&self) -> broadcast::Receiver<()> {
        self.change_tx.subscribe()
    }
}

fn default_host_binding() -> HostBinding {
    let cfg = crate::config::ensure_loaded();
    let prefs = agent_state::read_prefs();
    let agent_id = agent_state::resolve_default_agent(&prefs, &cfg);
    let profile_id = agent_state::resolve_default_profile(&prefs, &cfg, &agent_id)
        .map(|profile| normalize_launch_profile_id(Some(&profile)));
    HostBinding::new(agent_id, profile_id)
}

fn normalize_optional_launch_profile_id(profile_id: Option<&str>) -> Option<String> {
    profile_id
        .map(str::trim)
        .filter(|profile| !profile.is_empty())
        .map(|profile| normalize_launch_profile_id(Some(profile)))
}

fn launch_setting_profile_for_agent(agent_id: &str) -> Option<String> {
    let cfg = crate::config::ensure_loaded();
    let prefs = agent_state::read_prefs();
    agent_state::resolve_default_profile(&prefs, &cfg, agent_id)
        .map(|profile| normalize_launch_profile_id(Some(&profile)))
}

fn default_route_binding_and_workspace(route: &RouteKey) -> (HostBinding, PathBuf) {
    match channel_traits(&route.channel_kind).default_workspace {
        DefaultWorkspaceKind::General => {
            let cfg = crate::config::ensure_loaded();
            let host_binding = default_host_binding();
            (host_binding, cfg.resolve_workspace(""))
        }
        DefaultWorkspaceKind::ChannelDefault => {
            default_channel_binding_and_workspace(&route.channel_kind)
        }
    }
}

fn default_channel_binding_and_workspace(channel_kind: &str) -> (HostBinding, PathBuf) {
    let cfg = crate::config::ensure_loaded();
    let prefs = agent_state::read_prefs();
    let defaults = cfg.remote_channel_defaults(channel_kind);
    let agent_id = defaults
        .agent_id
        .filter(|agent| {
            cfg.enabled_agents.is_empty()
                || cfg
                    .enabled_agents
                    .iter()
                    .any(|enabled_agent| enabled_agent == agent)
        })
        .unwrap_or_else(|| agent_state::resolve_default_agent(&prefs, &cfg));
    let profile_id = defaults
        .profile_id
        .as_deref()
        .map(|profile| normalize_launch_profile_id(Some(profile)))
        .or_else(|| {
            agent_state::resolve_default_profile(&prefs, &cfg, &agent_id)
                .map(|profile| normalize_launch_profile_id(Some(&profile)))
        });
    let workspace = im_workspace_for_channel(&cfg, channel_kind);

    (HostBinding::new(agent_id, profile_id), workspace)
}

fn im_workspace_for_channel(cfg: &crate::config::Config, channel_kind: &str) -> PathBuf {
    cfg.resolve_workspace("")
        .join("im")
        .join(channel_workspace_segment(channel_kind))
}

fn channel_workspace_segment(channel_kind: &str) -> String {
    channel_kind
        .chars()
        .map(|ch| match ch {
            '/' | '\\' => '_',
            ch => ch,
        })
        .collect()
}

pub fn normalize_workspace_cwd(cwd: impl AsRef<Path>) -> PathBuf {
    let path = cwd.as_ref();
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|dir| dir.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    absolute.canonicalize().unwrap_or(absolute)
}

fn workspace_by_cwd<'a>(
    projection: &'a WorkspaceProjection,
    cwd: &Path,
) -> Option<&'a WorkspaceRecord> {
    projection.get_by_cwd(cwd).or_else(|| {
        projection
            .active()
            .find(|workspace| normalize_workspace_cwd(&workspace.cwd) == cwd)
    })
}

fn resolve_thread_session_alias(
    projection: &ThreadProjection,
    workspace_id: &WorkspaceId,
    host_binding: &HostBinding,
    requested_session_id: &str,
) -> anyhow::Result<Option<String>> {
    let exact_binding_matches = projection
        .for_workspace(workspace_id, true)
        .filter_map(|thread| thread.agent_sessions.get(host_binding))
        .flatten()
        .filter(|session| session_id_matches(&session.session_id, requested_session_id))
        .map(|session| session.session_id.clone())
        .collect::<Vec<_>>();
    if let Some(session_id) = unique_session_match(exact_binding_matches, requested_session_id)? {
        return Ok(Some(session_id));
    }

    let agent_matches = projection
        .for_workspace(workspace_id, true)
        .flat_map(|thread| thread.agent_sessions.values().flatten())
        .filter(|session| session.agent_id == host_binding.agent_id)
        .filter(|session| session_id_matches(&session.session_id, requested_session_id))
        .map(|session| session.session_id.clone())
        .collect::<Vec<_>>();
    if let Some(session_id) = unique_session_match(agent_matches, requested_session_id)? {
        return Ok(Some(session_id));
    }

    let thread_matches = projection
        .for_workspace(workspace_id, true)
        .filter(|thread| thread_id_matches(&thread.id, requested_session_id))
        .collect::<Vec<_>>();
    let Some(thread) = unique_thread_match(thread_matches, requested_session_id)? else {
        return Ok(None);
    };
    latest_host_session_for_thread(thread, host_binding)
        .map(Some)
        .ok_or_else(|| {
            anyhow!(
                "thread '{}' has no host session for agent '{}'",
                requested_session_id,
                host_binding.agent_id
            )
        })
}

fn latest_host_session_for_thread(
    thread: &WorkspaceThread,
    host_binding: &HostBinding,
) -> Option<String> {
    thread
        .agent_sessions
        .get(host_binding)
        .and_then(|sessions| sessions.last())
        .map(|session| session.session_id.clone())
        .or_else(|| {
            thread
                .agent_sessions
                .values()
                .flatten()
                .filter(|session| session.agent_id == host_binding.agent_id)
                .max_by(|a, b| a.observed_at.cmp(&b.observed_at))
                .map(|session| session.session_id.clone())
        })
}

fn unique_session_match(
    matches: Vec<String>,
    requested_session_id: &str,
) -> anyhow::Result<Option<String>> {
    let mut unique = Vec::new();
    for session_id in matches {
        if !unique.contains(&session_id) {
            unique.push(session_id);
        }
    }
    match unique.as_slice() {
        [] => Ok(None),
        [session_id] => Ok(Some(session_id.clone())),
        _ => Err(anyhow!(
            "session '{}' is ambiguous; use the full session id",
            requested_session_id
        )),
    }
}

fn unique_thread_match<'a>(
    matches: Vec<&'a WorkspaceThread>,
    requested_session_id: &str,
) -> anyhow::Result<Option<&'a WorkspaceThread>> {
    match matches.as_slice() {
        [] => Ok(None),
        [thread] => Ok(Some(*thread)),
        _ => Err(anyhow!(
            "thread '{}' is ambiguous; use the full thread id",
            requested_session_id
        )),
    }
}

fn session_id_matches(session_id: &str, requested_session_id: &str) -> bool {
    session_id == requested_session_id
        || crate::launch_sessions::short_id(session_id) == requested_session_id
}

fn thread_id_matches(thread_id: &WorkspaceThreadId, requested_session_id: &str) -> bool {
    thread_id.as_str() == requested_session_id
        || thread_id.as_str().chars().take(8).collect::<String>() == requested_session_id
}

fn runtime_has_started_host(state: &ThreadRuntimeState) -> bool {
    state.initialize.is_some() || state.busy || state.failed.is_some()
}

fn route_can_rehydrate_runtime(route: &RouteKey) -> bool {
    channel_traits(&route.channel_kind).rehydratable_runtime
}

fn is_legacy_channel_default_route(route: &RouteKey) -> bool {
    route.chat_id == LEGACY_CHANNEL_DEFAULT_CHAT_ID
}

#[allow(dead_code)]
fn workspace_name_from_path(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("Workspace")
        .to_string()
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use crate::workspace::threads::{
        MultiAgentTurnId, MultiAgentTurnMode, ThreadAgentId, ThreadAgentStatus,
    };

    use super::*;

    fn temp_paths() -> (PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("vibearound-wtm-{}", Uuid::new_v4()));
        (
            root.join("workspaces.jsonl"),
            root.join("threads.jsonl"),
            root.join("attachments.jsonl"),
        )
    }

    async fn seed_session_thread(
        manager: &WorkspaceThreadManager,
        root: PathBuf,
        agent_id: &str,
        profile_id: Option<&str>,
        session_id: &str,
        closed: bool,
    ) -> WorkspaceThreadId {
        let workspace = manager.ensure_workspace_for_cwd(root).await.unwrap();
        let profile_id = profile_id.map(ToOwned::to_owned);
        let host_binding = HostBinding::new(agent_id.to_string(), profile_id.clone());
        let thread = manager.new_thread_record_with_host(workspace.id.clone(), host_binding);
        manager.ensure_thread_persisted(&thread).await.unwrap();
        manager
            .thread_store
            .append(&ThreadEvent::agent_session_observed(
                thread.id.clone(),
                agent_id.to_string(),
                profile_id,
                session_id.to_string(),
            ))
            .await
            .unwrap();
        if closed {
            manager
                .thread_store
                .append(&ThreadEvent::closed(
                    thread.id.clone(),
                    Some("closed for test".to_string()),
                ))
                .await
                .unwrap();
        }
        thread.id
    }

    #[tokio::test]
    async fn route_resolves_to_stable_thread_attachment() {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let route = RouteKey::new("web", "chat-a");

        let first = manager.resolve_route_runtime(&route).await.unwrap();
        let second = manager.resolve_route_runtime(&route).await.unwrap();

        assert_eq!(
            first.state().await.thread_id,
            second.state().await.thread_id
        );
        assert_eq!(
            manager
                .current_attachment(&route)
                .await
                .unwrap()
                .unwrap()
                .workspace_id,
            WorkspaceId::general()
        );
    }

    #[tokio::test]
    async fn active_runtime_resolve_does_not_reload_thread_store() {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let route = RouteKey::new("feishu", "chat-a");

        let first = manager.resolve_route_runtime(&route).await.unwrap();
        let first_thread_id = first.state().await.thread_id;
        tokio::fs::write(manager.thread_store.path(), b"not valid jsonl\n")
            .await
            .unwrap();

        let second = manager.resolve_route_runtime(&route).await.unwrap();

        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(second.state().await.thread_id, first_thread_id);
    }

    #[tokio::test]
    async fn cancel_unattached_route_is_noop() {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let route = RouteKey::new("web", "chat-a");

        let cancelled = manager.cancel_route(&route).await.unwrap();

        assert!(!cancelled);
        assert!(manager.current_attachment(&route).await.unwrap().is_none());
        assert!(manager
            .workspace_store
            .read_events()
            .await
            .unwrap()
            .is_empty());
        assert!(manager.thread_store.read_events().await.unwrap().is_empty());
        assert!(manager
            .attachment_store
            .read_events()
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn channel_routes_get_route_private_threads() {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let first_route = RouteKey::new("feishu", "chat-a");
        let second_route = RouteKey::new("feishu", "chat-b");

        let first = manager.resolve_route_runtime(&first_route).await.unwrap();
        let second = manager.resolve_route_runtime(&second_route).await.unwrap();

        assert_ne!(
            first.state().await.thread_id,
            second.state().await.thread_id
        );
        assert_eq!(
            manager
                .current_attachment(&first_route)
                .await
                .unwrap()
                .unwrap()
                .thread_id,
            first.state().await.thread_id
        );
    }

    #[tokio::test]
    async fn different_channels_get_different_default_threads() {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let feishu = RouteKey::new("feishu", "chat-a");
        let slack = RouteKey::new("slack", "chat-a");

        let feishu_runtime = manager.resolve_route_runtime(&feishu).await.unwrap();
        let slack_runtime = manager.resolve_route_runtime(&slack).await.unwrap();

        assert_ne!(
            feishu_runtime.state().await.thread_id,
            slack_runtime.state().await.thread_id
        );
    }

    #[tokio::test]
    async fn stale_route_attachment_starts_new_thread() {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let route = RouteKey::new("feishu", "chat-a");
        let first = manager.resolve_route_runtime(&route).await.unwrap();
        let first_thread_id = first.state().await.thread_id;

        manager.runtimes.remove(&first_thread_id);
        let second = manager.resolve_route_runtime(&route).await.unwrap();

        assert_ne!(second.state().await.thread_id, first_thread_id);
        assert_eq!(
            manager
                .current_attachment(&route)
                .await
                .unwrap()
                .unwrap()
                .thread_id,
            second.state().await.thread_id
        );
    }

    #[tokio::test]
    async fn web_route_attachment_rehydrates_runtime_after_host_shutdown() {
        route_attachment_rehydrates_runtime_after_host_shutdown("web").await;
    }

    #[tokio::test]
    async fn tui_route_attachment_rehydrates_runtime_after_host_shutdown() {
        route_attachment_rehydrates_runtime_after_host_shutdown("tui").await;
    }

    async fn route_attachment_rehydrates_runtime_after_host_shutdown(channel_kind: &str) {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let route = RouteKey::new(channel_kind, "chat-a");
        let first = manager.resolve_route_runtime(&route).await.unwrap();
        let first_thread_id = first.state().await.thread_id;

        manager.shutdown_route_host(&route).await.unwrap();
        assert!(!manager.runtimes.contains_key(&first_thread_id));
        assert_eq!(
            manager
                .current_attachment(&route)
                .await
                .unwrap()
                .unwrap()
                .thread_id,
            first_thread_id
        );

        let second = manager.resolve_route_runtime(&route).await.unwrap();

        assert_eq!(second.state().await.thread_id, first_thread_id);
        assert!(manager.runtimes.contains_key(&first_thread_id));
    }

    #[tokio::test]
    async fn concurrent_route_resolve_uses_single_runtime() {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let route = RouteKey::new("feishu", "chat-a");

        let (first, second) = tokio::join!(
            manager.resolve_route_runtime(&route),
            manager.resolve_route_runtime(&route)
        );
        let first = first.unwrap();
        let second = second.unwrap();

        assert!(Arc::ptr_eq(&first, &second));
        let attached_events = manager
            .attachment_store
            .read_events()
            .await
            .unwrap()
            .into_iter()
            .filter(|event| {
                matches!(
                    event,
                    RouteAttachmentEvent::Attached {
                        route: attached_route,
                        ..
                    } if attached_route == &route
                )
            })
            .count();
        assert_eq!(attached_events, 1);
    }

    #[tokio::test]
    async fn detach_route_keeps_thread_open() {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let route = RouteKey::new("web", "chat-a");

        let runtime = manager.resolve_route_runtime(&route).await.unwrap();
        let thread_id = runtime.state().await.thread_id;

        manager.detach_route(&route).await.unwrap();

        assert!(manager.current_attachment(&route).await.unwrap().is_none());
        assert_eq!(
            manager.thread(&thread_id).await.unwrap().unwrap().status,
            crate::workspace::threads::store::ThreadStatus::Open
        );
    }

    #[tokio::test]
    async fn host_start_reset_keeps_other_routes_on_shared_thread() {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let stale_route = RouteKey::new("qqbot", "chat-old");
        let current_route = RouteKey::new("feishu", "chat-new");

        let runtime = manager.resolve_route_runtime(&stale_route).await.unwrap();
        let thread_id = runtime.state().await.thread_id;
        manager
            .attach_thread(&current_route, &thread_id)
            .await
            .unwrap();

        manager
            .reset_thread_attachments_for_host_start(&thread_id, Some(&current_route))
            .await
            .unwrap();

        let mut routes = manager
            .attached_routes_for_thread(&thread_id)
            .await
            .unwrap();
        routes.sort_by_key(|route| route.as_key());

        assert_eq!(routes, vec![current_route, stale_route]);
    }

    #[tokio::test]
    async fn runtime_entries_do_not_materialize_unstarted_threads() {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let route = RouteKey::new("web", "chat-a");

        let runtime = manager.resolve_route_runtime(&route).await.unwrap();
        let thread_id = runtime.state().await.thread_id;

        assert!(manager.runtime_entries().await.unwrap().is_empty());
        assert_eq!(
            manager.thread(&thread_id).await.unwrap().unwrap().status,
            crate::workspace::threads::store::ThreadStatus::Open
        );
    }

    #[tokio::test]
    async fn close_route_detaches_closed_thread() {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let route = RouteKey::new("slack", "chat-a");

        let runtime = manager.resolve_route_runtime(&route).await.unwrap();
        let thread_id = runtime.state().await.thread_id;

        manager
            .close_route(&route, Some("user closed".to_string()))
            .await
            .unwrap();

        assert!(manager.current_attachment(&route).await.unwrap().is_none());
        assert_eq!(
            manager.thread(&thread_id).await.unwrap().unwrap().status,
            crate::workspace::threads::store::ThreadStatus::Closed
        );
    }

    #[tokio::test]
    async fn switch_workspace_registers_existing_directory_path() {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let root = std::env::temp_dir().join(format!("vibearound-ws-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let route = RouteKey::new("slack", "chat-a");

        let runtime = manager
            .switch_workspace(&route, root.to_str().unwrap())
            .await
            .unwrap();

        assert_eq!(
            runtime.state().await.workspace,
            root.canonicalize().unwrap()
        );
        assert!(manager
            .workspace_projection()
            .await
            .unwrap()
            .get_by_cwd(&root.canonicalize().unwrap())
            .is_some());
    }

    #[tokio::test]
    async fn attach_external_session_normalizes_workspace_cwd() {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let root = std::env::temp_dir().join(format!("vibearound-ws-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let route = RouteKey::new("web", "chat-a");
        let thread_id = seed_session_thread(
            &manager,
            root.clone(),
            "codex",
            Some("direct"),
            "session-1",
            false,
        )
        .await;

        let runtime = manager
            .attach_external_session(
                &route,
                "codex".to_string(),
                Some("direct".to_string()),
                "session-1".to_string(),
                root.join("."),
                ExternalSessionAttachMode::ReuseOpenThread,
            )
            .await
            .unwrap();

        assert_eq!(
            runtime.state().await.workspace,
            root.canonicalize().unwrap()
        );
        assert_eq!(runtime.state().await.thread_id, thread_id);
        assert!(manager
            .workspace_projection()
            .await
            .unwrap()
            .get_by_cwd(&root.canonicalize().unwrap())
            .is_some());
    }

    #[tokio::test]
    async fn attach_external_session_keeps_missing_profile_unknown() {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let root = std::env::temp_dir().join(format!("vibearound-ws-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let route = RouteKey::new("feishu", "chat-a");
        let agent_id = format!("test-agent-{}", Uuid::new_v4());
        seed_session_thread(
            &manager,
            root.clone(),
            &agent_id,
            None,
            "external-session",
            false,
        )
        .await;

        let runtime = manager
            .attach_external_session(
                &route,
                agent_id.clone(),
                None,
                "external-session".to_string(),
                root,
                ExternalSessionAttachMode::ReuseOpenThread,
            )
            .await
            .unwrap();

        let state = runtime.state().await;
        assert_eq!(state.host_binding, HostBinding::new(agent_id, None));
        assert_eq!(state.session_id.as_deref(), Some("external-session"));
    }

    #[tokio::test]
    async fn attach_external_session_preserves_known_profile_when_missing() {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let root = std::env::temp_dir().join(format!("vibearound-ws-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let route = RouteKey::new("web", "chat-a");
        let thread_id = seed_session_thread(
            &manager,
            root.clone(),
            "claude",
            Some("deepseek-profile"),
            "external-session",
            false,
        )
        .await;

        let runtime = manager
            .attach_external_session(
                &route,
                "claude".to_string(),
                None,
                "external-session".to_string(),
                root,
                ExternalSessionAttachMode::ReuseOpenThread,
            )
            .await
            .unwrap();

        let state = runtime.state().await;
        assert_eq!(state.thread_id, thread_id);
        assert_eq!(
            state.host_binding,
            HostBinding::new("claude", Some("deepseek-profile".to_string()))
        );
    }

    #[tokio::test]
    async fn attach_external_session_rejects_unknown_session() {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let root = std::env::temp_dir().join(format!("vibearound-ws-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let route = RouteKey::new("feishu", "chat-a");

        let error = match manager
            .attach_external_session(
                &route,
                "codex".to_string(),
                Some("direct".to_string()),
                "missing-session".to_string(),
                root,
                ExternalSessionAttachMode::NewThread,
            )
            .await
        {
            Ok(_) => panic!("unknown session should be rejected"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("missing-session"));
        assert!(manager.current_attachment(&route).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn attach_external_session_reuses_existing_open_thread() {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let root = std::env::temp_dir().join(format!("vibearound-ws-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let web_route = RouteKey::new("web", "chat-a");
        let im_route = RouteKey::new("feishu", "chat-a");
        let thread_id = seed_session_thread(
            &manager,
            root.clone(),
            "codex",
            Some("direct"),
            "session-picked-up",
            false,
        )
        .await;
        manager.attach_thread(&web_route, &thread_id).await.unwrap();

        let runtime = manager
            .attach_external_session(
                &im_route,
                "codex".to_string(),
                Some("direct".to_string()),
                "session-picked-up".to_string(),
                root,
                ExternalSessionAttachMode::ReuseOpenThread,
            )
            .await
            .unwrap();

        assert_eq!(runtime.state().await.thread_id, thread_id);
        let mut routes = manager
            .attached_routes_for_thread(&thread_id)
            .await
            .unwrap();
        routes.sort_by_key(|route| route.as_key());
        assert_eq!(routes, vec![im_route, web_route]);
    }

    #[tokio::test]
    async fn attach_external_session_resolves_workspace_thread_id_to_host_session() {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let root = std::env::temp_dir().join(format!("vibearound-ws-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let route = RouteKey::new("web", "chat-a");
        let thread_id = seed_session_thread(
            &manager,
            root.clone(),
            "codex",
            Some("direct"),
            "native-session",
            false,
        )
        .await;

        let runtime = manager
            .attach_external_session(
                &route,
                "codex".to_string(),
                Some("direct".to_string()),
                thread_id.to_string(),
                root,
                ExternalSessionAttachMode::ReuseOpenThread,
            )
            .await
            .unwrap();

        let state = runtime.state().await;
        assert_eq!(state.thread_id, thread_id);
        assert_eq!(state.session_id.as_deref(), Some("native-session"));
    }

    #[tokio::test]
    async fn subagent_session_ids_for_agent_workspace_reads_thread_agents() {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let root = std::env::temp_dir().join(format!("vibearound-ws-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let workspace = manager
            .ensure_workspace_for_cwd(root.clone())
            .await
            .unwrap();
        let host_binding = HostBinding::new("codex", Some("direct".to_string()));
        let thread = manager.new_thread_record_with_host(workspace.id.clone(), host_binding);
        manager.ensure_thread_persisted(&thread).await.unwrap();

        let turn_id = MultiAgentTurnId::from("mat_a");
        let agent_id = ThreadAgentId::from("agent_a");
        let turn = MultiAgentTurn::new(
            turn_id.clone(),
            MultiAgentTurnMode::Parallel,
            vec![agent_id.clone()],
        );
        let agent = ThreadAgent::ready(
            agent_id.clone(),
            turn_id,
            "Builder",
            "codex",
            Some("direct".to_string()),
            "va/subagents/mat_a/builder",
            root.join("builder").to_string_lossy().to_string(),
            Some("build".to_string()),
        );
        manager
            .thread_store
            .append(&ThreadEvent::multi_agent_turn_initialized(
                thread.id.clone(),
                turn,
                vec![agent],
            ))
            .await
            .unwrap();
        manager
            .thread_store
            .append(&ThreadEvent::thread_agent_status_changed_with_session(
                thread.id,
                agent_id,
                ThreadAgentStatus::Running,
                Some("subagent-native-session".to_string()),
                None,
                None,
            ))
            .await
            .unwrap();

        let session_ids = manager
            .subagent_session_ids_for_agent_workspace("codex", &root)
            .await
            .unwrap();

        assert!(session_ids.contains("subagent-native-session"));
    }

    #[tokio::test]
    async fn attach_external_session_new_thread_mode_does_not_reuse_open_thread() {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let root = std::env::temp_dir().join(format!("vibearound-ws-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let existing_route = RouteKey::new("web", "chat-a");
        let switch_route = RouteKey::new("feishu", "chat-a");
        let existing_thread_id = seed_session_thread(
            &manager,
            root.clone(),
            "codex",
            Some("direct"),
            "session-switch",
            false,
        )
        .await;
        manager
            .attach_thread(&existing_route, &existing_thread_id)
            .await
            .unwrap();

        let runtime = manager
            .attach_external_session(
                &switch_route,
                "codex".to_string(),
                Some("direct".to_string()),
                "session-switch".to_string(),
                root,
                ExternalSessionAttachMode::NewThread,
            )
            .await
            .unwrap();

        assert_ne!(runtime.state().await.thread_id, existing_thread_id);
        assert_eq!(
            manager
                .current_attachment(&existing_route)
                .await
                .unwrap()
                .unwrap()
                .thread_id,
            existing_thread_id
        );
        assert_eq!(
            manager
                .current_attachment(&switch_route)
                .await
                .unwrap()
                .unwrap()
                .thread_id,
            runtime.state().await.thread_id
        );
    }

    #[tokio::test]
    async fn attach_external_session_creates_open_thread_when_matching_thread_is_closed() {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let root = std::env::temp_dir().join(format!("vibearound-ws-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let second_route = RouteKey::new("web", "chat-b");
        let thread_id = seed_session_thread(
            &manager,
            root.clone(),
            "codex",
            Some("direct"),
            "session-closed",
            true,
        )
        .await;

        let second = manager
            .attach_external_session(
                &second_route,
                "codex".to_string(),
                Some("direct".to_string()),
                "session-closed".to_string(),
                root,
                ExternalSessionAttachMode::ReuseOpenThread,
            )
            .await
            .unwrap();

        let second_thread_id = second.state().await.thread_id;
        assert_ne!(second_thread_id, thread_id);
        assert_eq!(
            manager.thread(&thread_id).await.unwrap().unwrap().status,
            ThreadStatus::Closed
        );
        let second_thread = manager
            .thread(&second_thread_id)
            .await
            .unwrap()
            .expect("second thread should exist");
        assert_eq!(second_thread.status, ThreadStatus::Open);
        assert!(second_thread.has_agent_session(
            &HostBinding::new("codex", Some("direct".to_string())),
            "session-closed"
        ));
        assert_eq!(
            second.state().await.session_id.as_deref(),
            Some("session-closed")
        );
        assert_eq!(
            manager
                .current_attachment(&second_route)
                .await
                .unwrap()
                .unwrap()
                .thread_id,
            second_thread_id
        );
    }

    #[tokio::test]
    async fn create_thread_for_cwd_starts_new_thread_even_when_workspace_has_threads() {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let root = std::env::temp_dir().join(format!("vibearound-ws-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let first_route = RouteKey::new("web", "chat-a");
        let second_route = RouteKey::new("web", "chat-b");

        let first = manager
            .create_thread_for_cwd(&first_route, root.clone())
            .await
            .unwrap();
        let second = manager
            .create_thread_for_cwd(&second_route, root.clone())
            .await
            .unwrap();

        assert_ne!(
            first.state().await.thread_id,
            second.state().await.thread_id
        );
        assert_eq!(
            first.state().await.workspace_id,
            second.state().await.workspace_id
        );
        assert_eq!(second.state().await.workspace, root.canonicalize().unwrap());
        assert_eq!(
            manager
                .current_attachment(&second_route)
                .await
                .unwrap()
                .unwrap()
                .thread_id,
            second.state().await.thread_id
        );
    }

    #[tokio::test]
    async fn switch_workspace_starts_new_thread_when_workspace_has_threads() {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let root = std::env::temp_dir().join(format!("vibearound-ws-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let route = RouteKey::new("feishu", "chat-a");

        let first_runtime = manager
            .switch_workspace(&route, root.to_str().unwrap())
            .await
            .unwrap();
        let second_runtime = manager
            .create_thread_in_current_workspace(&route)
            .await
            .unwrap();

        let third_runtime = manager
            .switch_workspace(&route, root.to_str().unwrap())
            .await
            .unwrap();

        assert_ne!(
            first_runtime.state().await.thread_id,
            third_runtime.state().await.thread_id
        );
        assert_ne!(
            second_runtime.state().await.thread_id,
            third_runtime.state().await.thread_id
        );
        assert_eq!(
            manager
                .current_attachment(&route)
                .await
                .unwrap()
                .unwrap()
                .thread_id,
            third_runtime.state().await.thread_id
        );
    }
}
