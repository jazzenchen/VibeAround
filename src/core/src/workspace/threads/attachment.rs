use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::routing::RouteKey;
use crate::storage::jsonl;

use super::super::registry::WorkspaceId;
use super::super::store::{event_id, now};
use super::store::WorkspaceThreadId;

const SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteAttachment {
    pub route: RouteKey,
    pub workspace_id: WorkspaceId,
    pub thread_id: WorkspaceThreadId,
    pub attached_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum RouteAttachmentEvent {
    Attached {
        schema_version: u8,
        event_id: String,
        occurred_at: String,
        route: RouteKey,
        workspace_id: WorkspaceId,
        thread_id: WorkspaceThreadId,
    },
    Detached {
        schema_version: u8,
        event_id: String,
        occurred_at: String,
        route: RouteKey,
    },
}

impl RouteAttachmentEvent {
    pub fn attached(
        route: RouteKey,
        workspace_id: impl Into<WorkspaceId>,
        thread_id: impl Into<WorkspaceThreadId>,
    ) -> Self {
        Self::Attached {
            schema_version: SCHEMA_VERSION,
            event_id: event_id(),
            occurred_at: now(),
            route,
            workspace_id: workspace_id.into(),
            thread_id: thread_id.into(),
        }
    }

    pub fn detached(route: RouteKey) -> Self {
        Self::Detached {
            schema_version: SCHEMA_VERSION,
            event_id: event_id(),
            occurred_at: now(),
            route,
        }
    }

    fn snapshot(attachment: &RouteAttachment) -> Self {
        Self::Attached {
            schema_version: SCHEMA_VERSION,
            event_id: event_id(),
            occurred_at: attachment.attached_at.clone(),
            route: attachment.route.clone(),
            workspace_id: attachment.workspace_id.clone(),
            thread_id: attachment.thread_id.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RouteAttachmentProjection {
    current: HashMap<RouteKey, RouteAttachment>,
}

impl RouteAttachmentProjection {
    pub fn from_events(events: &[RouteAttachmentEvent]) -> Self {
        let mut projection = Self::default();
        for event in events {
            projection.apply(event);
        }
        projection
    }

    pub fn apply(&mut self, event: &RouteAttachmentEvent) {
        match event {
            RouteAttachmentEvent::Attached {
                occurred_at,
                route,
                workspace_id,
                thread_id,
                ..
            } => {
                self.current.insert(
                    route.clone(),
                    RouteAttachment {
                        route: route.clone(),
                        workspace_id: workspace_id.clone(),
                        thread_id: thread_id.clone(),
                        attached_at: occurred_at.clone(),
                    },
                );
            }
            RouteAttachmentEvent::Detached { route, .. } => {
                self.current.remove(route);
            }
        }
    }

    pub fn get(&self, route: &RouteKey) -> Option<&RouteAttachment> {
        self.current.get(route)
    }

    pub fn all(&self) -> impl Iterator<Item = &RouteAttachment> {
        self.current.values()
    }

    pub fn len(&self) -> usize {
        self.current.len()
    }
}

#[derive(Debug, Clone)]
pub struct RouteAttachmentEventStore {
    path: PathBuf,
}

impl RouteAttachmentEventStore {
    pub fn default_path() -> PathBuf {
        crate::config::migrate_legacy_state_file("route-attachments.jsonl")
    }

    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn append(&self, event: &RouteAttachmentEvent) -> jsonl::Result<()> {
        jsonl::append(&self.path, event).await
    }

    pub async fn read_events(&self) -> jsonl::Result<Vec<RouteAttachmentEvent>> {
        jsonl::read_all(&self.path).await
    }

    pub async fn load_projection(&self) -> jsonl::Result<RouteAttachmentProjection> {
        let events = self.read_events().await?;
        let projection = RouteAttachmentProjection::from_events(&events);
        if events.len() > projection.len().saturating_add(16) {
            self.replace_with_projection(&projection).await?;
        }
        Ok(projection)
    }

    pub async fn compact(&self) -> jsonl::Result<()> {
        let projection = self.load_projection().await?;
        self.replace_with_projection(&projection).await
    }

    async fn replace_with_projection(
        &self,
        projection: &RouteAttachmentProjection,
    ) -> jsonl::Result<()> {
        let events = projection
            .all()
            .map(RouteAttachmentEvent::snapshot)
            .collect::<Vec<_>>();
        jsonl::replace_all(&self.path, &events).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_route_attachment_wins() {
        let route = RouteKey::with_bot_id("feishu", "bot-a", "chat-a");
        let events = vec![
            RouteAttachmentEvent::attached(route.clone(), "ws_a", "wt_a"),
            RouteAttachmentEvent::attached(route.clone(), "ws_b", "wt_b"),
        ];

        let projection = RouteAttachmentProjection::from_events(&events);
        let attachment = projection.get(&route).unwrap();

        assert_eq!(attachment.workspace_id, WorkspaceId::from("ws_b"));
        assert_eq!(attachment.thread_id, WorkspaceThreadId::from("wt_b"));
    }

    #[test]
    fn detach_clears_route_attachment() {
        let route = RouteKey::new("web", "chat-a");
        let events = vec![
            RouteAttachmentEvent::attached(route.clone(), "ws_a", "wt_a"),
            RouteAttachmentEvent::detached(route.clone()),
        ];

        let projection = RouteAttachmentProjection::from_events(&events);

        assert!(projection.get(&route).is_none());
    }

    #[tokio::test]
    async fn compact_removes_detached_attachment_history() {
        let path = std::env::temp_dir()
            .join(format!("vibearound-attachments-{}", uuid::Uuid::new_v4()))
            .join("attachments.jsonl");
        let store = RouteAttachmentEventStore::new(path.clone());
        let route = RouteKey::new("web", "chat-a");
        store
            .append(&RouteAttachmentEvent::attached(
                route.clone(),
                "ws_a",
                "wt_a",
            ))
            .await
            .unwrap();
        store
            .append(&RouteAttachmentEvent::detached(route.clone()))
            .await
            .unwrap();

        store.compact().await.unwrap();
        let events = store.read_events().await.unwrap();

        assert!(events.is_empty());

        let _ = tokio::fs::remove_dir_all(path.parent().unwrap()).await;
    }
}
