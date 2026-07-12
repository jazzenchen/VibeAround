//! Runtime owner for one workspace thread.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
use anyhow::Context;
use tokio::sync::{broadcast, mpsc, oneshot, watch, Mutex};
use tokio::time::{sleep, Duration};

use crate::agent::{Agent, AgentClientHandler, StartupSession};
use crate::routing::{channel_traits, wait_for_signal, ActiveTurnTarget, ChannelTarget, RouteKey};
use crate::workspace::registry::WorkspaceId;

use super::store::{
    HostBinding, MultiAgentTurn, ThreadAgent, ThreadAgentId, ThreadAgentStatus, ThreadEvent,
    ThreadEventStore, ThreadStatus, WorkspaceThread, WorkspaceThreadId,
};

#[derive(Debug, Clone)]
pub struct ThreadRuntimeState {
    pub thread_id: WorkspaceThreadId,
    pub workspace_id: WorkspaceId,
    pub host_binding: HostBinding,
    pub session_id: Option<String>,
    pub workspace: PathBuf,
    pub busy: bool,
    pub failed: Option<String>,
    pub initialize: Option<acp::InitializeResponse>,
    pub agents: Vec<ThreadAgent>,
    pub multi_agent_turns: Vec<MultiAgentTurn>,
}

pub(crate) struct PromptCancellation {
    cancel_rx: watch::Receiver<bool>,
    shutdown_rx: watch::Receiver<bool>,
}

#[path = "runtime_owner.rs"]
mod owner;
use owner::*;

#[path = "runtime_subagents.rs"]
mod subagents;

#[path = "runtime_events.rs"]
mod events;

#[derive(Clone)]
struct SubagentRuntime {
    agent: Arc<Agent>,
    session_id: String,
    client_handler: Arc<dyn AgentClientHandler>,
    active_turn_target: ActiveTurnTarget,
    completion_validator: Option<Arc<dyn SubagentCompletionValidator>>,
}

/// One live host-agent generation owned by a workspace thread.
///
/// The ACP session id remains on `ThreadRuntime` because it is durable across
/// generations. Everything tied to one connection/process is replaced as a
/// unit when the bridge reports that generation dead.
struct AcpSessionRunner {
    agent: Arc<Agent>,
    client_handler: Arc<dyn AgentClientHandler>,
}

impl AcpSessionRunner {
    fn is_live(&self) -> bool {
        self.agent.is_live()
    }

    async fn shutdown(self) {
        self.agent.shutdown().await;
    }
}

#[derive(Debug, Clone)]
pub struct SubagentCompletionResult {
    pub status: ThreadAgentStatus,
    pub last_error: Option<String>,
    pub report: Option<serde_json::Value>,
}

#[async_trait::async_trait]
pub trait SubagentCompletionValidator: Send + Sync + 'static {
    async fn reset_completion(&self);

    async fn validate_completion(&self) -> Result<SubagentCompletionResult, String>;
}

const SUBAGENT_START_MAX_ATTEMPTS: usize = 2;
const SUBAGENT_PROMPT_MAX_ATTEMPTS: usize = 2;
const SUBAGENT_RETRY_DELAY: Duration = Duration::from_millis(750);

pub struct ThreadRuntime {
    thread: Mutex<WorkspaceThread>,
    workspace: PathBuf,
    subagents: Mutex<BTreeMap<ThreadAgentId, SubagentRuntime>>,
    active_turn_target: ActiveTurnTarget,
    owner_tx: mpsc::UnboundedSender<ThreadOwnerCommand>,
    turn_state: watch::Receiver<TurnState>,
    activity_generation: AtomicU64,
    store: ThreadEventStore,
    change_tx: Option<broadcast::Sender<()>>,
}

impl ThreadRuntime {
    pub fn new(thread: WorkspaceThread, workspace: PathBuf, store: ThreadEventStore) -> Self {
        Self::with_change_tx(thread, workspace, store, None)
    }

    pub fn with_change_tx(
        thread: WorkspaceThread,
        workspace: PathBuf,
        store: ThreadEventStore,
        change_tx: Option<broadcast::Sender<()>>,
    ) -> Self {
        let session_id = latest_session_for_host(&thread);
        let (owner_tx, owner_rx) = mpsc::unbounded_channel();
        let (turn_state_tx, turn_state) = watch::channel(TurnState {
            session_id: session_id.clone(),
            ..TurnState::default()
        });
        tokio::spawn(
            ThreadOwner {
                command_rx: owner_rx,
                state_tx: turn_state_tx,
                change_tx: change_tx.clone(),
                host: None,
                session_id,
            }
            .run(),
        );
        Self {
            thread: Mutex::new(thread),
            workspace,
            subagents: Mutex::new(BTreeMap::new()),
            active_turn_target: ActiveTurnTarget::default(),
            owner_tx,
            turn_state,
            activity_generation: AtomicU64::new(0),
            store,
            change_tx,
        }
    }

    pub async fn state(&self) -> ThreadRuntimeState {
        let thread = self.thread.lock().await.clone();
        let turn_state = self.turn_state.borrow().clone();
        let initialize = turn_state
            .host_agent
            .as_ref()
            .filter(|agent| agent.is_live())
            .map(|agent| agent.initialize_response());
        ThreadRuntimeState {
            thread_id: thread.id.clone(),
            workspace_id: thread.workspace_id.clone(),
            host_binding: thread.host_binding.clone(),
            session_id: turn_state.session_id,
            workspace: self.workspace.clone(),
            busy: turn_state.busy,
            failed: turn_state.failed,
            initialize,
            agents: thread.agents.values().cloned().collect(),
            multi_agent_turns: thread.multi_agent_turns.values().cloned().collect(),
        }
    }

    pub fn active_turn_target(&self) -> ActiveTurnTarget {
        self.active_turn_target.clone()
    }

    /// Start the host agent and ensure a session exists, without sending a
    /// user prompt. This backs `/new` and route attachment warmup.
    pub async fn start(
        self: &Arc<Self>,
        route: &RouteKey,
        handler: Arc<dyn AgentClientHandler>,
    ) -> acp::Result<String> {
        self.mark_activity();
        let (reply, done) = oneshot::channel();
        self.owner_tx
            .send(ThreadOwnerCommand::Start(Box::new(StartCommand {
                runtime: Arc::clone(self),
                route: route.clone(),
                handler,
                reply,
            })))
            .map_err(|_| runtime_stopped_error())?;
        done.await.unwrap_or_else(|_| Err(runtime_stopped_error()))
    }

    pub async fn prompt(
        self: &Arc<Self>,
        target: &ChannelTarget,
        content_blocks: Vec<acp::ContentBlock>,
        handler: Arc<dyn AgentClientHandler>,
    ) -> acp::Result<acp::PromptResponse> {
        self.enqueue_prompt(target, content_blocks, handler, None)
            .await
    }

    pub(crate) async fn prompt_cancellable(
        self: &Arc<Self>,
        target: &ChannelTarget,
        content_blocks: Vec<acp::ContentBlock>,
        handler: Arc<dyn AgentClientHandler>,
        cancellation: PromptCancellation,
    ) -> acp::Result<acp::PromptResponse> {
        self.enqueue_prompt(target, content_blocks, handler, Some(cancellation))
            .await
    }

    async fn enqueue_prompt(
        self: &Arc<Self>,
        target: &ChannelTarget,
        content_blocks: Vec<acp::ContentBlock>,
        handler: Arc<dyn AgentClientHandler>,
        cancellation: Option<PromptCancellation>,
    ) -> acp::Result<acp::PromptResponse> {
        self.mark_activity();
        let (reply, done) = oneshot::channel();
        self.owner_tx
            .send(ThreadOwnerCommand::Prompt(Box::new(PromptCommand {
                runtime: Arc::clone(self),
                target: target.clone(),
                content_blocks,
                handler,
                cancellation,
                reply,
            })))
            .map_err(|_| acp::Error::new(-32603, "thread runtime stopped"))?;
        done.await
            .unwrap_or_else(|_| Err(acp::Error::new(-32603, "thread runtime stopped")))
    }

    pub async fn cancel(self: &Arc<Self>) -> acp::Result<()> {
        self.mark_activity();
        self.active_turn_target.cancel_current();
        {
            let subagents = self.subagents.lock().await;
            for subagent in subagents.values() {
                subagent.active_turn_target.cancel_current();
            }
        }
        let (reply, done) = oneshot::channel();
        self.owner_tx
            .send(ThreadOwnerCommand::Cancel(reply))
            .map_err(|_| runtime_stopped_error())?;
        done.await.unwrap_or_else(|_| Err(runtime_stopped_error()))
    }

    pub async fn close(self: &Arc<Self>, reason: Option<String>) -> acp::Result<()> {
        self.mark_activity();
        self.active_turn_target.cancel_current();
        let (reply, done) = oneshot::channel();
        self.owner_tx
            .send(ThreadOwnerCommand::Close(Box::new(CloseCommand {
                runtime: Arc::clone(self),
                reason,
                reply,
            })))
            .map_err(|_| runtime_stopped_error())?;
        done.await.unwrap_or_else(|_| Err(runtime_stopped_error()))
    }

    pub async fn shutdown_host(self: &Arc<Self>) {
        self.mark_activity();
        self.active_turn_target.cancel_current();
        let (reply, done) = oneshot::channel();
        if self
            .owner_tx
            .send(ThreadOwnerCommand::ShutdownHost(RuntimeCommand {
                runtime: Arc::clone(self),
                reply,
            }))
            .is_ok()
        {
            let _ = done.await;
        }
    }

    pub fn idle_generation(&self) -> u64 {
        self.activity_generation.load(Ordering::Relaxed)
    }

    pub async fn shutdown_host_if_idle(self: &Arc<Self>, generation: u64) -> bool {
        let (reply, done) = oneshot::channel();
        if self
            .owner_tx
            .send(ThreadOwnerCommand::ShutdownHostIfIdle {
                runtime: Arc::clone(self),
                generation,
                reply,
            })
            .is_err()
        {
            return false;
        }
        done.await.unwrap_or(false)
    }

    pub async fn switch_profile_preserving_session(
        self: &Arc<Self>,
        host_binding: HostBinding,
    ) -> acp::Result<()> {
        self.mark_activity();
        let (reply, done) = oneshot::channel();
        self.owner_tx
            .send(ThreadOwnerCommand::SwitchProfile(Box::new(
                SwitchProfileCommand {
                    runtime: Arc::clone(self),
                    host_binding,
                    reply,
                },
            )))
            .map_err(|_| runtime_stopped_error())?;
        done.await.unwrap_or_else(|_| Err(runtime_stopped_error()))
    }

    fn notify_change(&self) {
        if let Some(tx) = &self.change_tx {
            let _ = tx.send(());
        }
    }

    fn mark_activity(&self) {
        self.activity_generation.fetch_add(1, Ordering::Relaxed);
    }
}

async fn wait_for_prompt_cancellation(cancellation: &mut Option<PromptCancellation>) {
    match cancellation {
        Some(cancellation) => cancellation.cancelled().await,
        None => std::future::pending().await,
    }
}

fn runtime_stopped_error() -> acp::Error {
    acp::Error::new(-32603, "thread runtime stopped")
}

fn latest_session_for_host(thread: &WorkspaceThread) -> Option<String> {
    thread
        .agent_sessions
        .get(&thread.host_binding)
        .and_then(|sessions| sessions.last())
        .map(|session| session.session_id.clone())
}

fn host_startup_session(
    route: &RouteKey,
    runtime_session_id: Option<String>,
    thread: &WorkspaceThread,
) -> StartupSession {
    let Some(session_id) = runtime_session_id.or_else(|| latest_session_for_host(thread)) else {
        return StartupSession::Fresh;
    };
    if route_allows_startup_replay(route) {
        if thread.host_binding.agent_id == "gemini" {
            return StartupSession::ResumeOnly(session_id);
        }
        StartupSession::Load(session_id)
    } else {
        StartupSession::Resume(session_id)
    }
}

pub(crate) fn route_allows_startup_replay(route: &RouteKey) -> bool {
    channel_traits(&route.channel_kind).startup_replay
}

fn first_text(content_blocks: &[acp::ContentBlock]) -> Option<String> {
    content_blocks.iter().find_map(|block| match block {
        acp::ContentBlock::Text(text) => {
            let trimmed = text.text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.chars().take(240).collect())
            }
        }
        _ => None,
    })
}

fn aggregate_turn_status(
    agent_ids: &[ThreadAgentId],
    agents: &BTreeMap<ThreadAgentId, ThreadAgent>,
) -> ThreadAgentStatus {
    let statuses: Vec<ThreadAgentStatus> = agent_ids
        .iter()
        .filter_map(|agent_id| agents.get(agent_id).map(|agent| agent.status))
        .collect();
    if statuses.contains(&ThreadAgentStatus::Error) {
        ThreadAgentStatus::Error
    } else if statuses.contains(&ThreadAgentStatus::Running) {
        ThreadAgentStatus::Running
    } else if !statuses.is_empty()
        && statuses
            .iter()
            .all(|status| *status == ThreadAgentStatus::Completed)
    {
        ThreadAgentStatus::Completed
    } else {
        ThreadAgentStatus::Ready
    }
}

fn subagent_assignment_prompt(agent: &ThreadAgent) -> String {
    let assignment = serde_json::json!({
        "protocol": "va-agent-protocol",
        "kind": "assignment",
        "turn_id": agent.turn_id.to_string(),
        "to_agent_id": agent.id.to_string(),
        "task": agent.task.clone().unwrap_or_default(),
        "context": {
            "name": agent.name.clone(),
            "branch": agent.branch.clone(),
            "worktree": agent.worktree.clone(),
        }
    });
    subagent_assignment_prompt_from_value(agent, &assignment)
}

fn subagent_assignment_prompt_from_value(
    agent: &ThreadAgent,
    assignment: &serde_json::Value,
) -> String {
    let report_schema = subagent_report_schema(agent);
    format!(
        "You are a VibeAround subagent named {name}.\n\
         Work only inside your current git worktree. Do not merge branches or clean up worktrees.\n\
         Complete the assignment independently. You may stream progress and tool output normally.\n\
         When the assignment is complete, end your final assistant content with exactly one `va-agent-protocol` report envelope. Do not put any prose after that envelope.\n\
         The JSON inside the final report must match this report shape:\n\
         <va-agent-protocol>\n{report_schema}\n</va-agent-protocol>\n\n\
         <va-agent-protocol>\n{assignment}\n</va-agent-protocol>",
        name = agent.name.as_str(),
        report_schema =
            serde_json::to_string_pretty(&report_schema).unwrap_or_else(|_| report_schema.to_string()),
        assignment = serde_json::to_string_pretty(assignment).unwrap_or_else(|_| assignment.to_string())
    )
}

fn subagent_report_repair_prompt(agent: &ThreadAgent, error: &str) -> String {
    let report_schema = subagent_report_schema(agent);
    format!(
        "Your previous response could not be accepted as a VibeAround subagent report.\n\
         Reason: {error}\n\n\
         Do not continue task work. Emit exactly one final `va-agent-protocol` report envelope now. \
         Do not put any prose before or after the envelope.\n\
         The JSON inside the final report must match this report shape:\n\
         <va-agent-protocol>\n{report_schema}\n</va-agent-protocol>",
        error = error.trim(),
        report_schema =
            serde_json::to_string_pretty(&report_schema).unwrap_or_else(|_| report_schema.to_string()),
    )
}

fn subagent_new_session_request(agent: &ThreadAgent) -> acp::NewSessionRequest {
    acp::NewSessionRequest::new(PathBuf::from(agent.worktree.clone()))
        .meta(subagent_session_meta(agent))
}

fn subagent_session_meta(agent: &ThreadAgent) -> acp::Meta {
    let system_prompt = subagent_system_prompt(agent);
    let mut meta = serde_json::Map::new();
    meta.insert("systemPrompt".to_string(), serde_json::json!(system_prompt));
    meta.insert(
        "vibearound".to_string(),
        serde_json::json!({
            "role": "subagent",
            "system_prompt": system_prompt,
            "turn_id": agent.turn_id.to_string(),
            "subagent_id": agent.id.to_string(),
            "subagent_name": agent.name.clone(),
        }),
    );
    meta
}

fn subagent_system_prompt(agent: &ThreadAgent) -> String {
    format!(
        "You are a VibeAround subagent named {name}. Work only inside your assigned git worktree. \
         Treat host assignments wrapped in <va-agent-protocol> as control messages. \
         You may stream ordinary progress messages for the web UI, but control/report data must be wrapped in <va-agent-protocol>. \
         Your completion report envelope must be the final content you emit, with no prose after it. \
         Do not merge branches or clean up worktrees; the host reviews and merges results.",
        name = agent.name
    )
}

fn validate_subagent_assignment(
    agent: &ThreadAgent,
    agent_id: &ThreadAgentId,
    assignment: &serde_json::Value,
) -> acp::Result<()> {
    let object = assignment
        .as_object()
        .ok_or_else(|| acp::Error::new(-32602, "assignment must be a JSON object"))?;
    require_assignment_field(object, "protocol", "va-agent-protocol")?;
    require_assignment_field(object, "kind", "assignment")?;
    require_assignment_field(object, "turn_id", agent.turn_id.as_str())?;
    require_assignment_field(object, "to_agent_id", agent_id.as_str())?;
    let task = object
        .get("task")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|task| !task.is_empty())
        .ok_or_else(|| acp::Error::new(-32602, "assignment `task` must be a non-empty string"))?;
    if task.chars().count() > 24_000 {
        return Err(acp::Error::new(-32602, "assignment `task` is too large"));
    }
    Ok(())
}

fn require_assignment_field(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
    expected: &str,
) -> acp::Result<()> {
    let actual = object
        .get(field)
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            acp::Error::new(-32602, format!("assignment `{}` must be a string", field))
        })?;
    if actual == expected {
        Ok(())
    } else {
        Err(acp::Error::new(
            -32602,
            format!(
                "assignment `{}` expected `{}`, got `{}`",
                field, expected, actual
            ),
        ))
    }
}

fn subagent_report_schema(agent: &ThreadAgent) -> serde_json::Value {
    serde_json::json!({
        "protocol": "va-agent-protocol",
        "kind": "report",
        "turn_id": agent.turn_id.to_string(),
        "from_agent_id": agent.id.to_string(),
        "status": "completed",
        "summary": "One or two sentences describing the outcome.",
        "files_changed": ["relative/path.rs"],
        "tests": ["cargo test --manifest-path path/Cargo.toml"],
        "notes": ["Important caveats, blockers, or follow-up needed."]
    })
}

async fn append_thread_event(store: &ThreadEventStore, event: &ThreadEvent) -> acp::Result<()> {
    store
        .append(event)
        .await
        .with_context(|| format!("append thread event to {:?}", store.path()))
        .map_err(|error| acp::Error::new(-32603, format!("{:#}", error)))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::workspace::registry::WorkspaceId;

    fn test_thread_agent() -> ThreadAgent {
        ThreadAgent::ready(
            ThreadAgentId::from("00000000-0000-0000-0000-000000000001"),
            "mat_a",
            "John Planner",
            "codex",
            None,
            "va/subagents/mat_a/john-planner",
            "/tmp/john-planner",
            Some("plan".to_string()),
        )
    }

    fn thread_with_sessions() -> WorkspaceThread {
        thread_with_host_session("codex", Some("profile_a"), "session-old")
    }

    fn thread_with_host_session(
        agent_id: &str,
        profile_id: Option<&str>,
        session_id: &str,
    ) -> WorkspaceThread {
        let host = HostBinding::new(agent_id, profile_id.map(ToOwned::to_owned));
        let mut sessions = BTreeMap::new();
        sessions.insert(
            host.clone(),
            vec![super::super::store::AgentSessionRef {
                agent_id: agent_id.to_string(),
                profile_id: profile_id.map(ToOwned::to_owned),
                session_id: session_id.to_string(),
                observed_at: "2026-01-01T00:00:00.000Z".to_string(),
            }],
        );
        WorkspaceThread {
            id: WorkspaceThreadId::from("wt_a"),
            workspace_id: WorkspaceId::from("ws_a"),
            host_binding: host,
            status: ThreadStatus::Open,
            first_user_prompt: None,
            agent_sessions: sessions,
            agents: BTreeMap::new(),
            multi_agent_turns: BTreeMap::new(),
            created_at: "2026-01-01T00:00:00.000Z".to_string(),
            updated_at: "2026-01-01T00:00:00.000Z".to_string(),
        }
    }

    #[tokio::test]
    async fn runtime_initial_state_uses_latest_host_session() {
        let runtime = Arc::new(ThreadRuntime::new(
            thread_with_sessions(),
            PathBuf::from("/tmp/project"),
            ThreadEventStore::new("/tmp/unused.jsonl"),
        ));

        let state = runtime.state().await;

        assert_eq!(state.session_id.as_deref(), Some("session-old"));
    }

    #[tokio::test]
    async fn turn_owner_serializes_busy_state() {
        let runtime = Arc::new(ThreadRuntime::new(
            thread_with_sessions(),
            PathBuf::from("/tmp/project"),
            ThreadEventStore::new("/tmp/unused.jsonl"),
        ));
        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        runtime
            .owner_tx
            .send(ThreadOwnerCommand::Probe {
                started: started_tx,
                release: release_rx,
            })
            .unwrap();

        started_rx.await.expect("prompt scope started");
        assert!(runtime.state().await.busy);

        let mut turn_state = runtime.turn_state.clone();
        assert!(turn_state.borrow_and_update().busy);
        release_tx.send(()).unwrap();
        turn_state.changed().await.unwrap();

        assert!(!runtime.state().await.busy);
    }

    #[tokio::test]
    async fn cancel_signals_active_turn_before_agent_lookup() {
        let runtime = Arc::new(ThreadRuntime::new(
            thread_with_sessions(),
            PathBuf::from("/tmp/project"),
            ThreadEventStore::new("/tmp/unused.jsonl"),
        ));
        let _target_guard = runtime
            .active_turn_target
            .install(ChannelTarget::for_route(RouteKey::new("web", "chat-1")));
        let (_, mut cancelled) = runtime
            .active_turn_target
            .current_with_cancellation()
            .expect("active target");

        assert!(runtime.cancel().await.is_err(), "runtime has no live agent");
        cancelled
            .wait_for(|is_cancelled| *is_cancelled)
            .await
            .expect("active turn cancellation sender remains live");
    }

    #[test]
    fn web_routes_load_previous_host_session_for_playback() {
        let route = RouteKey::new("web", "chat-1");

        let startup_session = host_startup_session(&route, None, &thread_with_sessions());

        assert_eq!(
            startup_session,
            StartupSession::Load("session-old".to_string())
        );
    }

    #[test]
    fn tui_routes_load_previous_host_session_for_playback() {
        let route = RouteKey::new("tui", "chat-1");

        let startup_session = host_startup_session(&route, None, &thread_with_sessions());

        assert_eq!(
            startup_session,
            StartupSession::Load("session-old".to_string())
        );
    }

    #[test]
    fn web_gemini_routes_resume_without_load_fallback() {
        let route = RouteKey::new("web", "chat-1");
        let thread = thread_with_host_session("gemini", None, "gemini-session");

        let startup_session = host_startup_session(&route, None, &thread);

        assert_eq!(
            startup_session,
            StartupSession::ResumeOnly("gemini-session".to_string())
        );
    }

    #[test]
    fn im_routes_resume_previous_host_session_without_playback() {
        let route = RouteKey::new("slack", "dm-1");

        let startup_session = host_startup_session(
            &route,
            Some("runtime-session".to_string()),
            &thread_with_sessions(),
        );

        assert_eq!(
            startup_session,
            StartupSession::Resume("runtime-session".to_string())
        );
    }

    #[test]
    fn routes_without_known_session_start_fresh() {
        let route = RouteKey::new("slack", "dm-1");
        let thread = WorkspaceThread {
            id: WorkspaceThreadId::from("wt_a"),
            workspace_id: WorkspaceId::from("ws_a"),
            host_binding: HostBinding::new("codex", Some("direct".to_string())),
            status: ThreadStatus::Open,
            first_user_prompt: None,
            agent_sessions: BTreeMap::new(),
            agents: BTreeMap::new(),
            multi_agent_turns: BTreeMap::new(),
            created_at: "2026-01-01T00:00:00.000Z".to_string(),
            updated_at: "2026-01-01T00:00:00.000Z".to_string(),
        };

        let startup_session = host_startup_session(&route, None, &thread);

        assert_eq!(startup_session, StartupSession::Fresh);
    }

    #[tokio::test]
    async fn selecting_the_current_profile_is_a_noop() {
        let path = std::env::temp_dir().join(format!(
            "vibearound-runtime-noop-profile-{}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let runtime = Arc::new(ThreadRuntime::new(
            thread_with_sessions(),
            PathBuf::from("/tmp/project"),
            ThreadEventStore::new(&path),
        ));

        runtime
            .switch_profile_preserving_session(HostBinding::new(
                "codex",
                Some("profile_a".to_string()),
            ))
            .await
            .unwrap();

        assert_eq!(
            runtime.state().await.session_id.as_deref(),
            Some("session-old")
        );
        assert!(
            !path.exists(),
            "a no-op profile selection persisted a host change"
        );
    }

    #[test]
    fn first_text_is_trimmed_and_limited() {
        let long = format!("  {}  ", "a".repeat(300));
        let blocks = vec![acp::ContentBlock::Text(acp::TextContent::new(long))];

        let text = first_text(&blocks).unwrap();

        assert_eq!(text.len(), 240);
        assert!(text.chars().all(|c| c == 'a'));
    }

    #[test]
    fn validates_matching_follow_up_assignment() {
        let agent = test_thread_agent();
        let assignment = serde_json::json!({
            "protocol": "va-agent-protocol",
            "kind": "assignment",
            "turn_id": "mat_a",
            "to_agent_id": "00000000-0000-0000-0000-000000000001",
            "task": "Run another review pass.",
            "context": { "focus": "tests" }
        });

        validate_subagent_assignment(&agent, &agent.id, &assignment).unwrap();
    }

    #[test]
    fn rejects_follow_up_assignment_for_wrong_turn() {
        let agent = test_thread_agent();
        let assignment = serde_json::json!({
            "protocol": "va-agent-protocol",
            "kind": "assignment",
            "turn_id": "mat_other",
            "to_agent_id": "00000000-0000-0000-0000-000000000001",
            "task": "Run another review pass."
        });

        let error = validate_subagent_assignment(&agent, &agent.id, &assignment).unwrap_err();

        assert!(error.message.contains("turn_id"));
    }

    #[test]
    fn rejects_follow_up_assignment_without_task() {
        let agent = test_thread_agent();
        let assignment = serde_json::json!({
            "protocol": "va-agent-protocol",
            "kind": "assignment",
            "turn_id": "mat_a",
            "to_agent_id": "00000000-0000-0000-0000-000000000001",
            "task": " "
        });

        let error = validate_subagent_assignment(&agent, &agent.id, &assignment).unwrap_err();

        assert!(error.message.contains("task"));
    }

    #[test]
    fn repair_prompt_asks_only_for_protocol_report() {
        let agent = test_thread_agent();
        let prompt = subagent_report_repair_prompt(&agent, "missing report");

        assert!(prompt.contains("missing report"));
        assert!(prompt.contains("\"kind\": \"report\""));
        assert!(prompt.contains("\"from_agent_id\": \"00000000-0000-0000-0000-000000000001\""));
        assert!(prompt.contains("Do not continue task work"));
    }
}
