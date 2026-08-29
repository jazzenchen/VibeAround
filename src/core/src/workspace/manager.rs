//! Workspace/thread orchestration.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context};
use tokio::sync::{broadcast, Mutex};

use crate::agent::launch::normalize_launch_profile_id;
use crate::agent_state;
use crate::routing::{channel_traits, DefaultWorkspaceKind, RouteKey};

use super::normalize_platform_cwd;
use super::registry::{WorkspaceId, WorkspaceProjection, WorkspaceRecord, GENERAL_WORKSPACE_ID};
use super::runtime_registry::RuntimeRegistry;
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

#[path = "manager_previews.rs"]
mod previews;

const MAX_WARM_THREADS: usize = 4;
const WARM_THREAD_MIN_IDLE: Duration = Duration::from_secs(10 * 60);
const PREVIEW_WEB_CHAT_ID_PREFIX: &str = "ws_preview_";

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

pub fn preview_web_route_for_slug(slug: &str) -> RouteKey {
    RouteKey::new("web", format!("{PREVIEW_WEB_CHAT_ID_PREFIX}{slug}"))
}

pub fn preview_slug_from_web_route(route: &RouteKey) -> Option<&str> {
    if route.channel_kind != "web" || route.channel_instance_id != "web" {
        return None;
    }
    route
        .chat_id
        .strip_prefix(PREVIEW_WEB_CHAT_ID_PREFIX)
        .filter(|slug| !slug.is_empty())
}

pub struct WorkspaceThreadManager {
    workspace_store: WorkspaceEventStore,
    thread_store: ThreadEventStore,
    attachment_store: RouteAttachmentEventStore,
    runtimes: RuntimeRegistry,
    change_tx: broadcast::Sender<()>,
    preview_lifecycle: Mutex<()>,
    /// Threads the web has been promised but nobody has written to yet. They
    /// hold an id and a host binding and nothing else; the first prompt on
    /// their route is what makes them real.
    drafts: Mutex<HashMap<RouteKey, WorkspaceThread>>,
    /// VibeAround's MCP tools served over ACP to thread agents. Installed by
    /// the server at boot; thread handlers hand it to their ACP bridge.
    mcp_over_acp: std::sync::OnceLock<Arc<dyn crate::agent::AcpMcpServer>>,
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
            runtimes: RuntimeRegistry::new(),
            change_tx,
            preview_lifecycle: Mutex::new(()),
            drafts: Mutex::new(HashMap::new()),
            mcp_over_acp: std::sync::OnceLock::new(),
        })
    }

    /// Install the MCP-over-ACP server once at boot. Later calls are ignored.
    pub fn set_mcp_over_acp(&self, server: Arc<dyn crate::agent::AcpMcpServer>) {
        let _ = self.mcp_over_acp.set(server);
    }

    pub fn mcp_over_acp(&self) -> Option<Arc<dyn crate::agent::AcpMcpServer>> {
        self.mcp_over_acp.get().cloned()
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
            runtimes: RuntimeRegistry::new(),
            change_tx,
            preview_lifecycle: Mutex::new(()),
            drafts: Mutex::new(HashMap::new()),
            mcp_over_acp: std::sync::OnceLock::new(),
        })
    }

    pub async fn resolve_route_runtime(
        &self,
        route: &RouteKey,
    ) -> anyhow::Result<Arc<ThreadRuntime>> {
        if let Some(slug) = preview_slug_from_web_route(route) {
            let _preview_lifecycle = self.preview_lifecycle.lock().await;
            if let Some(runtime) = self.active_runtime_for_route(route).await? {
                return Ok(runtime);
            }
            return self.resolve_preview_route_runtime_locked(route, slug).await;
        }

        if let Some(runtime) = self.active_runtime_for_route(route).await? {
            return Ok(runtime);
        }
        if let Some(thread) = self.drafts.lock().await.remove(route) {
            self.ensure_thread_persisted(&thread).await?;
            self.attach_route(
                route.clone(),
                thread.workspace_id.clone(),
                thread.id.clone(),
            )
            .await?;
            return self.runtime_from_thread(thread).await;
        }

        let (host_binding, workspace_path) = default_route_binding_and_workspace(route);
        let workspace = self
            .ensure_default_workspace_for_route(route, workspace_path)
            .await?;
        let thread = self.new_thread_record_with_host(workspace.id.clone(), None, host_binding);
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
        let thread = self.new_thread_record_with_host(workspace.id.clone(), None, host_binding);
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
        let preview_slug = preview_slug_from_web_route(route).map(str::to_string);
        let _preview_lifecycle = if preview_slug.is_some() {
            Some(self.preview_lifecycle.lock().await)
        } else {
            None
        };
        let current = match self.active_runtime_for_route(route).await? {
            Some(runtime) => {
                let state = runtime.state().await;
                let thread = self
                    .thread(&state.thread_id)
                    .await?
                    .ok_or_else(|| anyhow!("thread {} not found", state.thread_id))?;
                let next_host_binding = host_binding_for_explicit_new(route, state.host_binding);
                runtime
                    .close(reason)
                    .await
                    .map_err(|error| anyhow!(error.to_string()))?;
                self.runtimes.remove(state.thread_id.clone());
                self.detach_route(route).await?;
                Some((thread, next_host_binding))
            }
            None => None,
        };

        if let Some((thread, host_binding)) = current {
            if let Some(preview_slug) = thread.preview_slug {
                return self
                    .create_preview_thread_for_route(
                        route,
                        thread.workspace_id,
                        thread.parent_thread_id,
                        preview_slug,
                        host_binding,
                    )
                    .await;
            }
            self.create_thread_for_route_with_host(route, thread.workspace_id, host_binding)
                .await
        } else if let Some(preview_slug) = preview_slug.as_deref() {
            self.resolve_preview_route_runtime_locked(route, preview_slug)
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
        let _preview_lifecycle = if preview_slug_from_web_route(route).is_some() {
            Some(self.preview_lifecycle.lock().await)
        } else {
            None
        };
        let Some(runtime) = self.active_runtime_for_route(route).await? else {
            return Ok(());
        };
        let thread_id = runtime.state().await.thread_id;
        runtime
            .close(reason)
            .await
            .map_err(|error| anyhow!(error.to_string()))?;
        self.runtimes.remove(thread_id.clone());
        self.detach_route(route).await?;
        Ok(())
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
        self.runtimes.remove(thread_id.clone());
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
        let Some(runtime) = self.runtimes.get(thread_id).await else {
            return Ok(());
        };
        runtime.shutdown_host().await;
        Ok(())
    }

    async fn ensure_general_workspace(&self, cwd: PathBuf) -> anyhow::Result<WorkspaceRecord> {
        let projection = self.workspace_projection().await?;
        if let Some(workspace) = projection.get(&WorkspaceId::general()) {
            return Ok(workspace.clone());
        }

        let cwd = normalize_workspace_cwd(cwd);
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
            DefaultWorkspaceKind::General => self.ensure_general_workspace(workspace_path).await,
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
        let event = match &thread.preview_slug {
            Some(preview_slug) => ThreadEvent::preview_created(
                thread.id.clone(),
                thread.workspace_id.clone(),
                thread.parent_thread_id.clone(),
                preview_slug.clone(),
                thread.host_binding.clone(),
            ),
            None => ThreadEvent::created(
                thread.id.clone(),
                thread.workspace_id.clone(),
                thread.parent_thread_id.clone(),
                thread.host_binding.clone(),
            ),
        };
        self.thread_store
            .append(&event)
            .await
            .context("append workspace thread")?;
        self.notify_change();
        Ok(())
    }

    fn new_thread_record_with_host(
        &self,
        workspace_id: WorkspaceId,
        parent_thread_id: Option<WorkspaceThreadId>,
        host_binding: HostBinding,
    ) -> WorkspaceThread {
        let event = ThreadEvent::created(
            WorkspaceThreadId::new(),
            workspace_id,
            parent_thread_id,
            host_binding,
        );
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
        if let Some(runtime) = self.runtimes.get(thread_id).await {
            return Ok(runtime);
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
        if let Some(runtime) = self.runtimes.get(&thread.id).await {
            return Ok(runtime);
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
        let registered = self
            .runtimes
            .get_or_insert(thread.id.clone(), Arc::clone(&runtime))
            .await;
        if !Arc::ptr_eq(&registered, &runtime) {
            return Ok(registered);
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

fn default_host_binding(
    cfg: &crate::config::Config,
    prefs: &agent_state::AgentsPrefsFile,
) -> HostBinding {
    let agent_id = agent_state::resolve_default_agent(prefs, cfg);
    let profile_id = agent_state::resolve_default_profile(prefs, cfg, &agent_id)
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
    let (cfg, prefs) = agent_state::read_config_and_prefs();
    agent_state::resolve_default_profile(&prefs, &cfg, agent_id)
        .map(|profile| normalize_launch_profile_id(Some(&profile)))
}

/// The profile a binding gets when no profile was named: the channel's own
/// configured profile, else the agent's default, which itself falls back to
/// the launch selection.
pub fn default_profile_for_agent(channel_kind: &str, agent_id: &str) -> Option<String> {
    let (cfg, prefs) = agent_state::read_config_and_prefs();
    default_profile_for_agent_from_settings(channel_kind, agent_id, &cfg, &prefs)
}

fn default_profile_for_agent_from_settings(
    channel_kind: &str,
    agent_id: &str,
    cfg: &crate::config::Config,
    prefs: &agent_state::AgentsPrefsFile,
) -> Option<String> {
    let profile_id = cfg
        .remote_channel_defaults(channel_kind)
        .profile_id
        .or_else(|| agent_state::resolve_default_profile(prefs, cfg, agent_id))?;
    Some(normalize_launch_profile_id(Some(&profile_id)))
}

fn default_route_binding_and_workspace(route: &RouteKey) -> (HostBinding, PathBuf) {
    let (cfg, prefs) = agent_state::read_config_and_prefs();
    default_route_binding_and_workspace_from_settings(route, &cfg, &prefs)
}

fn default_route_binding_and_workspace_from_settings(
    route: &RouteKey,
    cfg: &crate::config::Config,
    prefs: &agent_state::AgentsPrefsFile,
) -> (HostBinding, PathBuf) {
    match channel_traits(&route.channel_kind).default_workspace {
        DefaultWorkspaceKind::General => {
            let host_binding = default_host_binding(cfg, prefs);
            (host_binding, cfg.resolve_workspace(""))
        }
        DefaultWorkspaceKind::ChannelDefault => {
            default_channel_binding_and_workspace(&route.channel_kind, cfg, prefs)
        }
    }
}

fn host_binding_for_explicit_new(route: &RouteKey, current: HostBinding) -> HostBinding {
    match route.channel_kind.as_str() {
        "web" | "tui" => current,
        _ => default_route_binding_and_workspace(route).0,
    }
}

fn default_channel_binding_and_workspace(
    channel_kind: &str,
    cfg: &crate::config::Config,
    prefs: &agent_state::AgentsPrefsFile,
) -> (HostBinding, PathBuf) {
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
        .unwrap_or_else(|| agent_state::resolve_default_agent(prefs, cfg));
    let profile_id = default_profile_for_agent_from_settings(channel_kind, &agent_id, cfg, prefs);
    let workspace = im_workspace_for_channel(cfg, channel_kind);

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

#[cfg(test)]
#[path = "manager_tests.rs"]
mod tests;
