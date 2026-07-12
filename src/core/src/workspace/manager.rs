//! Workspace/thread orchestration.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context};
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use tokio::sync::broadcast;

use crate::agent::launch::normalize_launch_profile_id;
use crate::agent_state;
use crate::routing::{channel_traits, DefaultWorkspaceKind, RouteKey};

use super::normalize_platform_cwd;
use super::registry::{WorkspaceId, WorkspaceProjection, WorkspaceRecord, GENERAL_WORKSPACE_ID};
use super::store::{WorkspaceEvent, WorkspaceEventStore};
use super::threads::attachment::{
    RouteAttachmentEvent, RouteAttachmentEventStore, RouteAttachmentProjection,
    RouteAttachmentVisibility,
};
use super::threads::runtime::ThreadRuntime;
use super::threads::runtime::ThreadRuntimeState;
use super::threads::store::{
    HostBinding, MultiAgentTurn, ThreadAgent, ThreadEvent, ThreadEventStore, ThreadProjection,
    ThreadStatus, WorkspaceThread, WorkspaceThreadId,
};

#[path = "manager_sessions.rs"]
mod sessions;

#[path = "manager_routes.rs"]
mod routes;

pub const AGENT_HOST_IDLE_SHUTDOWN_DELAY: Duration = Duration::from_secs(10 * 60);

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
            change_tx,
        })
    }

    pub async fn resolve_route_runtime(
        &self,
        route: &RouteKey,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        self.adopt_legacy_route_attachment(route).await?;

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

    async fn adopt_legacy_route_attachment(&self, route: &RouteKey) -> anyhow::Result<()> {
        let legacy_route = RouteKey::new(&route.channel_kind, &route.chat_id);
        if route == &legacy_route
            || route
                .topic_id()
                .is_some_and(|topic_id| topic_id != route.chat_id)
            || self.current_attachment(route).await?.is_some()
        {
            return Ok(());
        }

        let Some(legacy_attachment) = self.current_attachment(&legacy_route).await? else {
            return Ok(());
        };

        self.attach_route(
            route.clone(),
            legacy_attachment.workspace_id,
            legacy_attachment.thread_id,
        )
        .await?;
        if let Err(error) = self.detach_route(&legacy_route).await {
            tracing::warn!(
                route = %route,
                legacy_route = %legacy_route,
                error = %error,
                "extended route adopted legacy attachment but legacy detach failed"
            );
        }
        Ok(())
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
    normalize_platform_cwd(absolute.canonicalize().unwrap_or(absolute))
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

#[allow(dead_code)]
fn workspace_name_from_path(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("Workspace")
        .to_string()
}

#[cfg(test)]
#[path = "manager_tests.rs"]
mod tests;
