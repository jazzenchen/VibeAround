use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::storage::jsonl;

use super::registry::{WorkspaceId, WorkspaceProjection};

const SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum WorkspaceEvent {
    Registered {
        schema_version: u8,
        event_id: String,
        occurred_at: String,
        workspace_id: WorkspaceId,
        cwd: PathBuf,
        name: String,
        is_general: bool,
    },
    Archived {
        schema_version: u8,
        event_id: String,
        occurred_at: String,
        workspace_id: WorkspaceId,
    },
}

impl WorkspaceEvent {
    pub fn registered(
        workspace_id: impl Into<WorkspaceId>,
        cwd: PathBuf,
        name: impl Into<String>,
        is_general: bool,
    ) -> Self {
        Self::Registered {
            schema_version: SCHEMA_VERSION,
            event_id: event_id(),
            occurred_at: now(),
            workspace_id: workspace_id.into(),
            cwd,
            name: name.into(),
            is_general,
        }
    }

    pub fn archived(workspace_id: impl Into<WorkspaceId>) -> Self {
        Self::Archived {
            schema_version: SCHEMA_VERSION,
            event_id: event_id(),
            occurred_at: now(),
            workspace_id: workspace_id.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct WorkspaceEventStore {
    path: PathBuf,
    io_lock: Arc<Mutex<()>>,
}

impl WorkspaceEventStore {
    pub fn default_path() -> PathBuf {
        crate::config::state_file("workspaces.jsonl")
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

    pub async fn append(&self, event: &WorkspaceEvent) -> jsonl::Result<()> {
        let _guard = self.io_lock.lock().await;
        jsonl::append(&self.path, event).await
    }

    pub async fn read_events(&self) -> jsonl::Result<Vec<WorkspaceEvent>> {
        let _guard = self.io_lock.lock().await;
        jsonl::read_all(&self.path).await
    }

    pub async fn load_projection(&self) -> Result<WorkspaceProjection, WorkspaceStoreLoadError> {
        let events = self.read_events().await?;
        Ok(WorkspaceProjection::from_events(&events)?)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceStoreLoadError {
    #[error(transparent)]
    Jsonl(#[from] jsonl::JsonlError),
    #[error(transparent)]
    Projection(#[from] super::registry::WorkspaceProjectionError),
}

pub(crate) fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub(crate) fn event_id() -> String {
    format!("evt_{}", Uuid::new_v4().simple())
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    fn temp_jsonl_path() -> PathBuf {
        std::env::temp_dir()
            .join(format!("vibearound-workspace-store-{}", Uuid::new_v4()))
            .join("workspaces.jsonl")
    }

    #[tokio::test]
    async fn store_round_trips_workspace_events() {
        let path = temp_jsonl_path();
        let store = WorkspaceEventStore::new(path.clone());
        let event = WorkspaceEvent::registered(
            WorkspaceId::general(),
            PathBuf::from("/tmp/general"),
            "General",
            true,
        );

        store.append(&event).await.unwrap();

        let projection = store.load_projection().await.unwrap();
        let workspace = projection.get(&WorkspaceId::general()).unwrap();
        assert_eq!(workspace.cwd, PathBuf::from("/tmp/general"));
        assert!(workspace.is_general);

        let _ = tokio::fs::remove_dir_all(path.parent().unwrap()).await;
    }

    #[tokio::test]
    async fn cloned_stores_serialize_appends() {
        let path = temp_jsonl_path();
        let store = WorkspaceEventStore::new(path.clone());
        let mut tasks = tokio::task::JoinSet::new();
        for index in 0..16 {
            let store = store.clone();
            tasks.spawn(async move {
                store
                    .append(&WorkspaceEvent::registered(
                        format!("ws_{index}"),
                        PathBuf::from(format!("/tmp/ws-{index}")),
                        format!("Workspace {index}"),
                        false,
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
}
