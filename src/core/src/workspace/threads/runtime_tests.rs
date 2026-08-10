use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use super::*;
use crate::workspace::registry::WorkspaceId;

struct NoopClientHandler;

#[async_trait::async_trait]
impl AgentClientHandler for NoopClientHandler {
    async fn session_notification(&self, _args: acp::SessionNotification) -> acp::Result<()> {
        Ok(())
    }

    async fn request_permission(
        &self,
        _args: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        Ok(acp::RequestPermissionResponse::new(
            acp::RequestPermissionOutcome::Cancelled,
        ))
    }
}

#[test]
fn cancelled_prompt_is_not_a_successful_completion() {
    let completed = Ok(acp::PromptResponse::new(acp::StopReason::EndTurn));
    let cancelled = Ok(acp::PromptResponse::new(acp::StopReason::Cancelled));
    let failed = Err(acp::Error::method_not_found());

    assert!(prompt_completed_successfully(&completed));
    assert!(!prompt_completed_successfully(&cancelled));
    assert!(!prompt_completed_successfully(&failed));
}

#[tokio::test]
async fn cancelled_prompt_returns_real_result_without_shutdown_within_grace() {
    let (reply, result) = oneshot::channel();
    reply.send(42).unwrap();
    let prompt = async { result.await.unwrap() };
    tokio::pin!(prompt);
    let shutdown_called = Arc::new(AtomicBool::new(false));
    let shutdown_flag = Arc::clone(&shutdown_called);

    let result = await_cancelled_prompt(
        prompt.as_mut(),
        Duration::from_secs(1),
        Duration::from_secs(1),
        move || async move {
            shutdown_flag.store(true, Ordering::SeqCst);
        },
    )
    .await;

    assert_eq!(result, Some(42));
    assert!(!shutdown_called.load(Ordering::SeqCst));
}

#[tokio::test]
async fn cancelled_prompt_forces_shutdown_after_grace_then_waits_for_result() {
    let (reply, result) = oneshot::channel();
    let prompt = async { result.await.unwrap() };
    tokio::pin!(prompt);
    let shutdown_called = Arc::new(AtomicBool::new(false));
    let shutdown_flag = Arc::clone(&shutdown_called);

    let result = await_cancelled_prompt(
        prompt.as_mut(),
        Duration::from_millis(1),
        Duration::from_secs(1),
        move || async move {
            shutdown_flag.store(true, Ordering::SeqCst);
            reply.send(7).unwrap();
        },
    )
    .await;

    assert_eq!(result, Some(7));
    assert!(shutdown_called.load(Ordering::SeqCst));
}

#[tokio::test]
async fn cancelled_prompt_stops_waiting_after_shutdown_grace() {
    let prompt = std::future::pending::<u8>();
    tokio::pin!(prompt);
    let shutdown_called = Arc::new(AtomicBool::new(false));
    let shutdown_flag = Arc::clone(&shutdown_called);

    let result = await_cancelled_prompt(
        prompt.as_mut(),
        Duration::from_millis(1),
        Duration::from_millis(1),
        move || async move {
            shutdown_flag.store(true, Ordering::SeqCst);
        },
    )
    .await;

    assert_eq!(result, None);
    assert!(shutdown_called.load(Ordering::SeqCst));
}

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
        parent_thread_id: None,
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
async fn dropping_runtime_closes_the_owner_command_channel() {
    let runtime = Arc::new(ThreadRuntime::new(
        thread_with_sessions(),
        PathBuf::from("/tmp/project"),
        ThreadEventStore::new("/tmp/unused.jsonl"),
    ));
    let owner = runtime.owner_tx.downgrade();

    drop(runtime);

    assert!(owner.upgrade().is_none());
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

    let (ping_tx, ping_rx) = oneshot::channel();
    runtime
        .owner_tx
        .send(ThreadOwnerCommand::Ping(ping_tx))
        .unwrap();
    ping_rx
        .await
        .expect("owner should consume events while a turn is active");

    let mut turn_state = runtime.turn_state.clone();
    assert!(turn_state.borrow_and_update().busy);
    release_tx.send(()).unwrap();
    turn_state.changed().await.unwrap();

    assert!(!runtime.state().await.busy);
}

#[tokio::test]
async fn queued_start_honors_cancellation_before_agent_launch() {
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
    started_rx.await.unwrap();

    let (cancel_tx, cancellation) = watch::channel(false);
    let (reply_tx, start) = oneshot::channel();
    runtime
        .owner_tx
        .send(ThreadOwnerCommand::Start(Box::new(StartCommand {
            runtime: Arc::clone(&runtime),
            route: RouteKey::new("web", "chat-1"),
            handler: Arc::new(NoopClientHandler),
            cancellation: Some(cancellation),
            reply: reply_tx,
        })))
        .unwrap();
    let (ping_tx, ping_rx) = oneshot::channel();
    runtime
        .owner_tx
        .send(ThreadOwnerCommand::Ping(ping_tx))
        .unwrap();
    ping_rx.await.expect("start command was not queued");

    cancel_tx.send_replace(true);
    release_tx.send(()).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(1), start)
        .await
        .expect("cancelled start did not finish")
        .unwrap()
        .unwrap();

    assert!(result.is_none());
    assert!(runtime.state().await.initialize.is_none());
}

#[tokio::test]
async fn prompt_completion_refreshes_thread_activity() {
    let runtime = Arc::new(ThreadRuntime::new(
        thread_with_sessions(),
        PathBuf::from("/tmp/project"),
        ThreadEventStore::new("/tmp/unused.jsonl"),
    ));
    let before = runtime.thread_activity();
    let (reply_tx, reply_rx) = oneshot::channel();

    runtime
        .owner_tx
        .send(ThreadOwnerCommand::PromptFinished {
            result: Box::new(Ok(acp::PromptResponse::new(acp::StopReason::EndTurn))),
            reply: reply_tx,
        })
        .unwrap();
    reply_rx.await.unwrap().unwrap();

    let after = runtime.thread_activity();
    assert!(after.generation > before.generation);
    assert!(after.last_activity_at >= before.last_activity_at);
}

#[tokio::test]
async fn touch_does_not_emit_a_global_runtime_change() {
    let (change_tx, mut change_rx) = broadcast::channel(1);
    let runtime = Arc::new(ThreadRuntime::with_change_tx(
        thread_with_sessions(),
        PathBuf::from("/tmp/project"),
        ThreadEventStore::new("/tmp/unused.jsonl"),
        Some(change_tx),
    ));
    let before = runtime.thread_activity();
    runtime.mark_activity();
    let (ping_tx, ping_rx) = oneshot::channel();
    runtime
        .owner_tx
        .send(ThreadOwnerCommand::Ping(ping_tx))
        .unwrap();
    ping_rx.await.unwrap();

    let after = runtime.thread_activity();
    assert!(after.generation > before.generation);
    assert!(change_rx.try_recv().is_err());
}

#[tokio::test]
async fn thread_events_are_consumed_while_a_turn_is_active() {
    let path = std::env::temp_dir().join(format!(
        "vibearound-runtime-owner-events-{}.jsonl",
        uuid::Uuid::new_v4()
    ));
    let runtime = Arc::new(ThreadRuntime::new(
        thread_with_sessions(),
        PathBuf::from("/tmp/project"),
        ThreadEventStore::new(&path),
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
    started_rx.await.unwrap();

    let agent = test_thread_agent();
    let turn = MultiAgentTurn::new(
        agent.turn_id.clone(),
        super::super::store::MultiAgentTurnMode::Parallel,
        vec![agent.id.clone()],
    );
    tokio::time::timeout(
        Duration::from_secs(1),
        runtime.initialize_multi_agent_turn(turn, vec![agent.clone()]),
    )
    .await
    .expect("thread event waited behind the active turn")
    .unwrap();

    assert_eq!(runtime.state().await.agents, vec![agent]);
    release_tx.send(()).unwrap();
    let _ = tokio::fs::remove_file(path).await;
}

#[tokio::test]
async fn cancel_signals_active_turn_without_agent_lookup() {
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
    let (started_tx, started_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    runtime
        .owner_tx
        .send(ThreadOwnerCommand::Probe {
            started: started_tx,
            release: release_rx,
        })
        .unwrap();
    started_rx.await.unwrap();

    runtime.cancel().await.unwrap();
    cancelled
        .wait_for(|is_cancelled| *is_cancelled)
        .await
        .expect("active turn cancellation sender remains live");
    release_tx.send(()).unwrap();
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
        parent_thread_id: None,
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
        .switch_profile_preserving_session(HostBinding::new("codex", Some("profile_a".to_string())))
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
