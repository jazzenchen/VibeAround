use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::storage::jsonl;

use super::super::registry::WorkspaceId;
use super::super::store::{event_id, now};

const SCHEMA_VERSION: u8 = 1;

#[path = "store_model.rs"]
mod model;
pub use model::*;

#[path = "store_projection.rs"]
mod projection;
pub use projection::*;

#[derive(Debug, Clone)]
pub struct ThreadEventStore {
    path: PathBuf,
    io_lock: Arc<Mutex<()>>,
}

impl ThreadEventStore {
    pub fn default_path() -> PathBuf {
        crate::config::migrate_legacy_state_file("workspace-threads.jsonl")
    }

    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            io_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn append(&self, event: &ThreadEvent) -> jsonl::Result<()> {
        let _guard = self.io_lock.lock().await;
        jsonl::append(&self.path, event).await
    }

    pub async fn read_events(&self) -> jsonl::Result<Vec<ThreadEvent>> {
        let _guard = self.io_lock.lock().await;
        jsonl::read_all(&self.path).await
    }

    pub async fn load_projection(&self) -> Result<ThreadProjection, ThreadStoreLoadError> {
        let events = self.read_events().await?;
        Ok(ThreadProjection::from_events(&events)?)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ThreadStoreLoadError {
    #[error(transparent)]
    Jsonl(#[from] jsonl::JsonlError),
    #[error(transparent)]
    Projection(#[from] ThreadProjectionError),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_jsonl_path() -> PathBuf {
        std::env::temp_dir()
            .join(format!("vibearound-thread-store-{}", Uuid::new_v4()))
            .join("workspace-threads.jsonl")
    }

    #[tokio::test]
    async fn cloned_stores_serialize_appends() {
        let path = temp_jsonl_path();
        let store = ThreadEventStore::new(path.clone());
        let mut tasks = tokio::task::JoinSet::new();
        for index in 0..16 {
            let store = store.clone();
            tasks.spawn(async move {
                store
                    .append(&ThreadEvent::created(
                        format!("wt_{index}"),
                        format!("ws_{index}"),
                        HostBinding::new("codex", None),
                    ))
                    .await
            });
        }
        while let Some(result) = tasks.join_next().await {
            result.unwrap().unwrap();
        }

        assert_eq!(store.read_events().await.unwrap().len(), 16);
        let _ = tokio::fs::remove_dir_all(path.parent().unwrap()).await;
    }

    #[test]
    fn projection_tracks_thread_lifecycle() {
        let thread_id = WorkspaceThreadId::from("wt_a");
        let workspace_id = WorkspaceId::from("ws_a");
        let codex = HostBinding::new("codex", Some("profile_a".to_string()));
        let claude = HostBinding::new("claude", None);
        let events = vec![
            ThreadEvent::created(thread_id.clone(), workspace_id.clone(), codex.clone()),
            ThreadEvent::first_user_prompt_set(thread_id.clone(), "build this"),
            ThreadEvent::agent_session_observed(
                thread_id.clone(),
                "codex",
                Some("profile_a".to_string()),
                "session-1",
            ),
            ThreadEvent::host_changed(thread_id.clone(), claude.clone()),
            ThreadEvent::multi_agent_turn_initialized(
                thread_id.clone(),
                MultiAgentTurn::new(
                    "mat_a",
                    MultiAgentTurnMode::Parallel,
                    vec![ThreadAgentId::from("00000000-0000-0000-0000-000000000001")],
                ),
                vec![ThreadAgent::ready(
                    "00000000-0000-0000-0000-000000000001",
                    "mat_a",
                    "John Planner",
                    "codex",
                    None,
                    "va/subagents/mat_a/john-planner",
                    "/tmp/john-planner",
                    Some("plan".to_string()),
                )],
            ),
            ThreadEvent::closed(thread_id.clone(), Some("done".to_string())),
        ];

        let projection = ThreadProjection::from_events(&events).unwrap();
        let thread = projection.get(&thread_id).unwrap();

        assert_eq!(thread.workspace_id, workspace_id);
        assert_eq!(thread.host_binding, claude);
        assert_eq!(thread.status, ThreadStatus::Closed);
        assert_eq!(thread.first_user_prompt.as_deref(), Some("build this"));
        assert_eq!(
            thread.agent_sessions.get(&codex).unwrap()[0].session_id,
            "session-1"
        );
        assert_eq!(thread.multi_agent_turns.len(), 1);
        assert_eq!(thread.agents.len(), 1);
    }

    #[test]
    fn first_prompt_is_not_overwritten() {
        let thread_id = WorkspaceThreadId::from("wt_a");
        let events = vec![
            ThreadEvent::created(
                thread_id.clone(),
                WorkspaceId::from("ws_a"),
                HostBinding::new("codex", None),
            ),
            ThreadEvent::first_user_prompt_set(thread_id.clone(), "first"),
            ThreadEvent::first_user_prompt_set(thread_id.clone(), "second"),
        ];

        let projection = ThreadProjection::from_events(&events).unwrap();

        assert_eq!(
            projection
                .get(&thread_id)
                .unwrap()
                .first_user_prompt
                .as_deref(),
            Some("first")
        );
    }

    #[test]
    fn web_lifecycle_close_events_do_not_close_threads() {
        let thread_id = WorkspaceThreadId::from("wt_a");
        for reason in [None, Some("web idle timeout"), Some("web resume aborted")] {
            let events = vec![
                ThreadEvent::created(
                    thread_id.clone(),
                    WorkspaceId::from("ws_a"),
                    HostBinding::new("codex", Some("direct".to_string())),
                ),
                ThreadEvent::closed(thread_id.clone(), reason.map(str::to_string)),
            ];

            let projection = ThreadProjection::from_events(&events).unwrap();

            assert_eq!(
                projection.get(&thread_id).unwrap().status,
                ThreadStatus::Open
            );
        }
    }

    #[test]
    fn agent_status_updates_aggregate_turn_status() {
        let thread_id = WorkspaceThreadId::from("wt_a");
        let turn_id = MultiAgentTurnId::from("mat_a");
        let first_id = ThreadAgentId::from("00000000-0000-0000-0000-000000000001");
        let second_id = ThreadAgentId::from("00000000-0000-0000-0000-000000000002");
        let events = vec![
            ThreadEvent::created(
                thread_id.clone(),
                WorkspaceId::from("ws_a"),
                HostBinding::new("codex", None),
            ),
            ThreadEvent::multi_agent_turn_initialized(
                thread_id.clone(),
                MultiAgentTurn::new(
                    turn_id.clone(),
                    MultiAgentTurnMode::Parallel,
                    vec![first_id.clone(), second_id.clone()],
                ),
                vec![
                    ThreadAgent::ready(
                        first_id.clone(),
                        turn_id.clone(),
                        "John Planner",
                        "codex",
                        None,
                        "va/subagents/mat_a/john-planner",
                        "/tmp/john-planner",
                        Some("plan".to_string()),
                    ),
                    ThreadAgent::ready(
                        second_id.clone(),
                        turn_id.clone(),
                        "Jane Builder",
                        "codex",
                        None,
                        "va/subagents/mat_a/jane-builder",
                        "/tmp/jane-builder",
                        Some("build".to_string()),
                    ),
                ],
            ),
            ThreadEvent::thread_agent_status_changed_with_session(
                thread_id.clone(),
                first_id.clone(),
                ThreadAgentStatus::Running,
                Some("subagent-session-1".to_string()),
                None,
                None,
            ),
            ThreadEvent::thread_agent_status_changed(
                thread_id.clone(),
                first_id.clone(),
                ThreadAgentStatus::Completed,
                None,
                None,
            ),
            ThreadEvent::thread_agent_status_changed(
                thread_id,
                second_id,
                ThreadAgentStatus::Completed,
                None,
                None,
            ),
        ];

        let projection = ThreadProjection::from_events(&events).unwrap();
        let thread = projection.get(&WorkspaceThreadId::from("wt_a")).unwrap();

        assert_eq!(
            thread.multi_agent_turns.get(&turn_id).unwrap().status,
            ThreadAgentStatus::Completed
        );
        assert_eq!(
            thread.agents.get(&first_id).unwrap().session_id.as_deref(),
            Some("subagent-session-1")
        );
    }

    #[test]
    fn duplicate_agent_session_observations_are_idempotent() {
        let thread_id = WorkspaceThreadId::from("wt_a");
        let codex = HostBinding::new("codex", Some("profile_a".to_string()));
        let events = vec![
            ThreadEvent::created(thread_id.clone(), WorkspaceId::from("ws_a"), codex.clone()),
            ThreadEvent::agent_session_observed(
                thread_id.clone(),
                "codex",
                Some("profile_a".to_string()),
                "session-1",
            ),
            ThreadEvent::agent_session_observed(
                thread_id.clone(),
                "codex",
                Some("profile_a".to_string()),
                "session-1",
            ),
        ];

        let projection = ThreadProjection::from_events(&events).unwrap();
        let thread = projection.get(&thread_id).unwrap();

        assert_eq!(thread.agent_sessions.get(&codex).unwrap().len(), 1);
    }
}
