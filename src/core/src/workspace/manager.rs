//! Workspace/thread orchestration.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context};
use dashmap::DashMap;
use tokio::sync::broadcast;

use crate::agent_state;
use crate::routing::RouteKey;

use super::registry::{WorkspaceId, WorkspaceProjection, WorkspaceRecord, GENERAL_WORKSPACE_ID};
use super::store::{WorkspaceEvent, WorkspaceEventStore};
use super::threads::attachment::{
    RouteAttachmentEvent, RouteAttachmentEventStore, RouteAttachmentProjection,
};
use super::threads::runtime::ThreadRuntime;
use super::threads::runtime::ThreadRuntimeState;
use super::threads::store::{
    HostBinding, MultiAgentTurn, ThreadAgent, ThreadEvent, ThreadEventStore, ThreadProjection,
    WorkspaceThread, WorkspaceThreadId,
};

pub const AGENT_HOST_IDLE_SHUTDOWN_DELAY: Duration = Duration::from_secs(10 * 60);

pub struct WorkspaceThreadManager {
    workspace_store: WorkspaceEventStore,
    thread_store: ThreadEventStore,
    attachment_store: RouteAttachmentEventStore,
    runtimes: DashMap<WorkspaceThreadId, Arc<ThreadRuntime>>,
    pending_selections: DashMap<RouteKey, Vec<ThreadChoice>>,
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
            pending_selections: DashMap::new(),
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
            pending_selections: DashMap::new(),
            change_tx,
        })
    }

    pub async fn resolve_route_runtime(
        &self,
        route: &RouteKey,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        if let Some(attached) = self.current_attachment(route).await? {
            return self.runtime_for_thread(&attached.thread_id).await;
        }

        let workspace = self.ensure_general_workspace().await?;
        let thread = self
            .latest_open_thread(&workspace.id)
            .await?
            .unwrap_or_else(|| self.new_thread_record(workspace.id.clone()));
        self.ensure_thread_persisted(&thread).await?;
        self.attach_route(route.clone(), workspace.id, thread.id.clone())
            .await?;
        self.runtime_from_thread(thread).await
    }

    pub async fn create_thread_for_route(
        &self,
        route: &RouteKey,
        workspace_id: WorkspaceId,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        let workspace = self
            .workspace(&workspace_id)
            .await?
            .ok_or_else(|| anyhow!("workspace {} not found", workspace_id))?;
        let thread = self.new_thread_record(workspace.id.clone());
        self.ensure_thread_persisted(&thread).await?;
        self.attach_route(route.clone(), workspace.id, thread.id.clone())
            .await?;
        self.runtime_from_thread(thread).await
    }

    pub async fn create_thread_in_current_workspace(
        &self,
        route: &RouteKey,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        let workspace_id = self
            .current_attachment(route)
            .await?
            .map(|attachment| attachment.workspace_id)
            .unwrap_or_else(|| WorkspaceId::general());
        let workspace_id = if self.workspace(&workspace_id).await?.is_some() {
            workspace_id
        } else {
            self.ensure_general_workspace().await?.id
        };
        self.create_thread_for_route(route, workspace_id).await
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
        let Some(attached) = self.current_attachment(route).await? else {
            return Ok(());
        };
        let runtime = self.runtime_for_thread(&attached.thread_id).await?;
        runtime
            .close(reason)
            .await
            .map_err(|error| anyhow!(error.to_string()))?;
        self.runtimes.remove(&attached.thread_id);
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
        self.pending_selections.remove(route);
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
        let Some(attached) = self.current_attachment(route).await? else {
            return Ok(());
        };
        self.shutdown_thread_host(&attached.thread_id).await
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
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        let workspace = self.ensure_workspace_for_cwd(cwd).await?;
        let host_binding = HostBinding::new(agent_id.clone(), profile_id.clone());
        let projection = self.thread_projection().await?;
        let thread = projection
            .for_workspace(&workspace.id, true)
            .find(|thread| {
                thread
                    .agent_sessions
                    .values()
                    .flatten()
                    .any(|session| session.session_id == session_id)
            })
            .cloned()
            .unwrap_or_else(|| {
                self.new_thread_record_with_host(workspace.id.clone(), host_binding.clone())
            });

        self.ensure_thread_persisted(&thread).await?;
        if thread.status != crate::workspace::threads::store::ThreadStatus::Closed
            && thread.host_binding != host_binding
        {
            self.thread_store
                .append(&ThreadEvent::host_changed(
                    thread.id.clone(),
                    host_binding.clone(),
                    false,
                ))
                .await
                .context("append external session host binding")?;
            self.notify_change();
        }
        if thread.status != crate::workspace::threads::store::ThreadStatus::Closed
            && !thread.has_agent_session(&host_binding, &session_id)
        {
            self.thread_store
                .append(&ThreadEvent::agent_session_observed(
                    thread.id.clone(),
                    agent_id,
                    profile_id,
                    session_id,
                ))
                .await
                .context("append external session")?;
            self.notify_change();
        }
        self.attach_route(route.clone(), workspace.id, thread.id.clone())
            .await?;
        let thread = self
            .thread(&thread.id)
            .await?
            .ok_or_else(|| anyhow!("thread {} not found after attach", thread.id))?;
        self.runtime_from_thread(thread).await
    }

    pub async fn switch_workspace(
        &self,
        route: &RouteKey,
        token: &str,
    ) -> anyhow::Result<WorkspaceSwitch> {
        let workspace = self
            .resolve_workspace(token)
            .await?
            .ok_or_else(|| anyhow!("workspace '{}' not found", token))?;
        let threads = self.open_threads_for_workspace(&workspace.id).await?;
        if threads.is_empty() {
            let runtime = self.create_thread_for_route(route, workspace.id).await?;
            return Ok(WorkspaceSwitch::Started(runtime));
        }
        let choices: Vec<ThreadChoice> = threads.into_iter().map(ThreadChoice::from).collect();
        self.pending_selections
            .insert(route.clone(), choices.clone());
        Ok(WorkspaceSwitch::NeedsSelection {
            workspace,
            threads: choices,
        })
    }

    pub async fn select_pending_thread(
        &self,
        route: &RouteKey,
        text: &str,
    ) -> anyhow::Result<PendingThreadSelection> {
        let Some((_, choices)) = self.pending_selections.remove(route) else {
            return Ok(PendingThreadSelection::NoPending);
        };
        let token = text.trim();
        let selected = parse_thread_choice(token, &choices);
        match selected {
            Some(thread_id) => self
                .attach_thread(route, &thread_id)
                .await
                .map(PendingThreadSelection::Selected),
            None => {
                self.pending_selections
                    .insert(route.clone(), choices.clone());
                Ok(PendingThreadSelection::Invalid { threads: choices })
            }
        }
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
            .map(|attachment| attachment.route.clone())
            .collect())
    }

    pub async fn runtime_entries(&self) -> anyhow::Result<Vec<WorkspaceThreadRuntimeEntry>> {
        let thread_projection = self.thread_projection().await?;
        let attachment_projection = self.attachment_projection().await?;
        let mut routes_by_thread: HashMap<WorkspaceThreadId, RouteKey> = HashMap::new();
        for attachment in attachment_projection.all() {
            routes_by_thread.insert(attachment.thread_id.clone(), attachment.route.clone());
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
            entries.push(WorkspaceThreadRuntimeEntry {
                route: routes_by_thread.get(&thread.id).cloned(),
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
        if let Some(attached) = self.current_attachment(route).await? {
            self.schedule_host_idle_shutdown(attached.thread_id);
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

        let cwd = normalize_workspace_cwd(crate::config::builtin_workspaces_dir());
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

    async fn latest_open_thread(
        &self,
        workspace_id: &WorkspaceId,
    ) -> anyhow::Result<Option<WorkspaceThread>> {
        Ok(self
            .open_threads_for_workspace(workspace_id)
            .await?
            .into_iter()
            .max_by(|a, b| a.updated_at.cmp(&b.updated_at)))
    }

    async fn open_threads_for_workspace(
        &self,
        workspace_id: &WorkspaceId,
    ) -> anyhow::Result<Vec<WorkspaceThread>> {
        Ok(self
            .thread_projection()
            .await?
            .for_workspace(workspace_id, false)
            .cloned()
            .collect())
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

    fn new_thread_record(&self, workspace_id: WorkspaceId) -> WorkspaceThread {
        let host_binding = default_host_binding();
        self.new_thread_record_with_host(workspace_id, host_binding)
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
        self.runtimes.insert(thread.id.clone(), Arc::clone(&runtime));
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

fn parse_thread_choice(token: &str, choices: &[ThreadChoice]) -> Option<WorkspaceThreadId> {
    if let Ok(index) = token.parse::<usize>() {
        if index > 0 {
            return choices
                .get(index - 1)
                .map(|choice| choice.thread_id.clone());
        }
    }
    choices
        .iter()
        .find(|choice| choice.thread_id.as_str() == token)
        .map(|choice| choice.thread_id.clone())
}

#[derive(Debug, Clone)]
pub struct ThreadChoice {
    pub thread_id: WorkspaceThreadId,
    pub host_binding: HostBinding,
    pub updated_at: String,
    pub first_user_prompt: Option<String>,
}

impl From<WorkspaceThread> for ThreadChoice {
    fn from(thread: WorkspaceThread) -> Self {
        Self {
            thread_id: thread.id,
            host_binding: thread.host_binding,
            updated_at: thread.updated_at,
            first_user_prompt: thread.first_user_prompt,
        }
    }
}

pub enum WorkspaceSwitch {
    Started(Arc<ThreadRuntime>),
    NeedsSelection {
        workspace: WorkspaceRecord,
        threads: Vec<ThreadChoice>,
    },
}

pub enum PendingThreadSelection {
    NoPending,
    Selected(Arc<ThreadRuntime>),
    Invalid { threads: Vec<ThreadChoice> },
}

fn default_host_binding() -> HostBinding {
    let cfg = crate::config::ensure_loaded();
    let prefs = agent_state::read_prefs();
    let agent_id = agent_state::resolve_default_agent(&prefs, &cfg);
    let profile_id = agent_state::resolve_default_profile(&prefs, &cfg, &agent_id)
        .or(Some("direct".to_string()));
    HostBinding::new(agent_id, profile_id)
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

fn runtime_has_started_host(state: &ThreadRuntimeState) -> bool {
    state.initialize.is_some() || state.busy || state.failed.is_some()
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

    use super::*;

    fn temp_paths() -> (PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("vibearound-wtm-{}", Uuid::new_v4()));
        (
            root.join("workspaces.jsonl"),
            root.join("threads.jsonl"),
            root.join("attachments.jsonl"),
        )
    }

    #[tokio::test]
    async fn route_resolves_to_stable_thread_attachment() {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let route = RouteKey::new("feishu", "chat-a");

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

        let switch = manager
            .switch_workspace(&route, root.to_str().unwrap())
            .await
            .unwrap();

        let WorkspaceSwitch::Started(runtime) = switch else {
            panic!("new workspace should start a thread immediately");
        };
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

        let runtime = manager
            .attach_external_session(
                &route,
                "codex".to_string(),
                Some("direct".to_string()),
                "session-1".to_string(),
                root.join("."),
            )
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
    async fn attach_external_session_reuses_closed_matching_thread() {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let root = std::env::temp_dir().join(format!("vibearound-ws-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let first_route = RouteKey::new("web", "chat-a");
        let second_route = RouteKey::new("web", "chat-b");

        let first = manager
            .attach_external_session(
                &first_route,
                "codex".to_string(),
                Some("direct".to_string()),
                "session-closed".to_string(),
                root.clone(),
            )
            .await
            .unwrap();
        let thread_id = first.state().await.thread_id;

        manager
            .close_thread(&thread_id, Some("user closed".to_string()))
            .await
            .unwrap();

        let second = manager
            .attach_external_session(
                &second_route,
                "codex".to_string(),
                Some("direct".to_string()),
                "session-closed".to_string(),
                root,
            )
            .await
            .unwrap();

        assert_eq!(second.state().await.thread_id, thread_id);
        assert_eq!(
            manager.thread(&thread_id).await.unwrap().unwrap().status,
            crate::workspace::threads::store::ThreadStatus::Closed
        );
        assert_eq!(
            manager
                .current_attachment(&second_route)
                .await
                .unwrap()
                .unwrap()
                .thread_id,
            thread_id
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
    async fn invalid_pending_thread_selection_is_consumed() {
        let (workspaces, threads, attachments) = temp_paths();
        let manager = WorkspaceThreadManager::with_paths(workspaces, threads, attachments);
        let root = std::env::temp_dir().join(format!("vibearound-ws-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let route = RouteKey::new("feishu", "chat-a");

        let first = manager
            .switch_workspace(&route, root.to_str().unwrap())
            .await
            .unwrap();
        let WorkspaceSwitch::Started(first_runtime) = first else {
            panic!("new workspace should start first thread");
        };
        let second_runtime = manager
            .create_thread_in_current_workspace(&route)
            .await
            .unwrap();

        let switch = manager
            .switch_workspace(&route, root.to_str().unwrap())
            .await
            .unwrap();
        let WorkspaceSwitch::NeedsSelection { threads, .. } = switch else {
            panic!("existing workspace should ask for thread selection");
        };
        assert_eq!(threads.len(), 2);

        let invalid = manager
            .select_pending_thread(&route, "not-a-thread")
            .await
            .unwrap();
        assert!(matches!(invalid, PendingThreadSelection::Invalid { .. }));
        assert_eq!(
            manager
                .current_attachment(&route)
                .await
                .unwrap()
                .unwrap()
                .thread_id,
            second_runtime.state().await.thread_id
        );

        let first_thread_id = first_runtime.state().await.thread_id;
        let selected = manager
            .select_pending_thread(&route, first_thread_id.as_str())
            .await
            .unwrap();
        let PendingThreadSelection::Selected(runtime) = selected else {
            panic!("expected pending selection to survive invalid input");
        };
        assert_eq!(runtime.state().await.thread_id, first_thread_id);
    }
}
