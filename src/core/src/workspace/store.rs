use std::path::{Path, PathBuf};

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
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
}

impl WorkspaceEventStore {
    pub fn default_path() -> PathBuf {
        crate::config::migrate_legacy_state_file("workspaces.jsonl")
    }

    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn append(&self, event: &WorkspaceEvent) -> jsonl::Result<()> {
        jsonl::append(&self.path, event).await
    }

    pub async fn read_events(&self) -> jsonl::Result<Vec<WorkspaceEvent>> {
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
}
