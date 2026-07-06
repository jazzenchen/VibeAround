use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use super::store::WorkspaceEvent;

pub const GENERAL_WORKSPACE_ID: &str = "general";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkspaceId(String);

impl WorkspaceId {
    pub fn new() -> Self {
        Self(format!("ws_{}", Uuid::new_v4().simple()))
    }

    pub fn general() -> Self {
        Self(GENERAL_WORKSPACE_ID.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for WorkspaceId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<String> for WorkspaceId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for WorkspaceId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl std::fmt::Display for WorkspaceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceRecord {
    pub id: WorkspaceId,
    pub cwd: PathBuf,
    pub name: String,
    pub is_general: bool,
    pub archived: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceProjection {
    by_id: BTreeMap<WorkspaceId, WorkspaceRecord>,
    active_by_cwd: BTreeMap<PathBuf, WorkspaceId>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WorkspaceProjectionError {
    #[error("workspace id {workspace_id} is already registered")]
    DuplicateWorkspaceId { workspace_id: WorkspaceId },
    #[error("cwd {cwd:?} is already bound to workspace {existing_workspace_id}")]
    DuplicateCwd {
        cwd: PathBuf,
        existing_workspace_id: WorkspaceId,
    },
    #[error("workspace {workspace_id} does not exist")]
    UnknownWorkspace { workspace_id: WorkspaceId },
}

impl WorkspaceProjection {
    pub fn from_events(events: &[WorkspaceEvent]) -> Result<Self, WorkspaceProjectionError> {
        let mut projection = Self::default();
        for event in events {
            projection.apply(event)?;
        }
        Ok(projection)
    }

    pub fn apply(&mut self, event: &WorkspaceEvent) -> Result<(), WorkspaceProjectionError> {
        match event {
            WorkspaceEvent::Registered {
                occurred_at,
                workspace_id,
                cwd,
                name,
                is_general,
                ..
            } => self.register(
                workspace_id.clone(),
                cwd.clone(),
                name.clone(),
                *is_general,
                occurred_at.clone(),
            ),
            WorkspaceEvent::Archived {
                occurred_at,
                workspace_id,
                ..
            } => self.archive(workspace_id, occurred_at),
        }
    }

    pub fn get(&self, id: &WorkspaceId) -> Option<&WorkspaceRecord> {
        self.by_id.get(id)
    }

    pub fn get_by_cwd(&self, cwd: &Path) -> Option<&WorkspaceRecord> {
        self.active_by_cwd
            .get(cwd)
            .and_then(|id| self.by_id.get(id))
    }

    pub fn all(&self) -> impl Iterator<Item = &WorkspaceRecord> {
        self.by_id.values()
    }

    pub fn active(&self) -> impl Iterator<Item = &WorkspaceRecord> {
        self.by_id.values().filter(|workspace| !workspace.archived)
    }

    fn register(
        &mut self,
        workspace_id: WorkspaceId,
        cwd: PathBuf,
        name: String,
        is_general: bool,
        occurred_at: String,
    ) -> Result<(), WorkspaceProjectionError> {
        let cwd = super::normalize_platform_cwd(cwd);
        if self.by_id.contains_key(&workspace_id) {
            return Err(WorkspaceProjectionError::DuplicateWorkspaceId { workspace_id });
        }
        if let Some(existing_workspace_id) = self.active_by_cwd.get(&cwd) {
            return Err(WorkspaceProjectionError::DuplicateCwd {
                cwd,
                existing_workspace_id: existing_workspace_id.clone(),
            });
        }

        let record = WorkspaceRecord {
            id: workspace_id.clone(),
            cwd: cwd.clone(),
            name,
            is_general,
            archived: false,
            created_at: occurred_at.clone(),
            updated_at: occurred_at,
        };
        self.by_id.insert(workspace_id.clone(), record);
        self.active_by_cwd.insert(cwd, workspace_id);
        Ok(())
    }

    fn archive(
        &mut self,
        workspace_id: &WorkspaceId,
        occurred_at: &str,
    ) -> Result<(), WorkspaceProjectionError> {
        let record = self.by_id.get_mut(workspace_id).ok_or_else(|| {
            WorkspaceProjectionError::UnknownWorkspace {
                workspace_id: workspace_id.clone(),
            }
        })?;
        record.archived = true;
        record.updated_at = occurred_at.to_string();
        self.active_by_cwd.remove(&record.cwd);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registered(id: impl Into<WorkspaceId>, cwd: &str) -> WorkspaceEvent {
        WorkspaceEvent::registered(id, PathBuf::from(cwd), "Project", false)
    }

    #[test]
    fn projection_enforces_unique_cwd() {
        let events = vec![
            registered("ws_a", "/tmp/project"),
            registered("ws_b", "/tmp/project"),
        ];

        let error = WorkspaceProjection::from_events(&events).unwrap_err();

        assert!(matches!(
            error,
            WorkspaceProjectionError::DuplicateCwd { .. }
        ));
    }

    #[test]
    fn archive_removes_active_cwd_binding() {
        let workspace_id = WorkspaceId::from("ws_a");
        let cwd = PathBuf::from("/tmp/project");
        let events = vec![
            WorkspaceEvent::registered(workspace_id.clone(), cwd.clone(), "Project", false),
            WorkspaceEvent::archived(workspace_id.clone()),
        ];

        let projection = WorkspaceProjection::from_events(&events).unwrap();

        assert!(projection.get(&workspace_id).unwrap().archived);
        assert!(projection.get_by_cwd(&cwd).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn projection_normalizes_windows_verbatim_cwd() {
        let workspace_id = WorkspaceId::from("ws_a");
        let events = vec![WorkspaceEvent::registered(
            workspace_id.clone(),
            PathBuf::from(r"\\?\D:\_P\26\test_VibeAround"),
            "Project",
            false,
        )];

        let projection = WorkspaceProjection::from_events(&events).unwrap();
        let cwd = PathBuf::from(r"D:\_P\26\test_VibeAround");

        assert_eq!(projection.get(&workspace_id).unwrap().cwd, cwd);
        assert!(projection.get_by_cwd(&cwd).is_some());
    }
}
