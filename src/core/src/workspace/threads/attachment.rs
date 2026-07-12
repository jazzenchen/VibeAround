use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::{mpsc, oneshot};

use crate::routing::RouteKey;
use crate::storage::jsonl;

use super::super::registry::WorkspaceId;
use super::super::store::{event_id, now};
use super::store::WorkspaceThreadId;

const SCHEMA_VERSION: u8 = 2;
const LEGACY_CHANNEL_DEFAULT_CHAT_ID: &str = "__channel_default__";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteAttachmentVisibility {
    #[default]
    Visible,
    Hidden,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteAttachment {
    pub route: RouteKey,
    pub workspace_id: WorkspaceId,
    pub thread_id: WorkspaceThreadId,
    pub attached_at: String,
    pub visibility: RouteAttachmentVisibility,
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
        #[serde(default)]
        visibility: RouteAttachmentVisibility,
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
            visibility: RouteAttachmentVisibility::Visible,
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
            visibility: attachment.visibility,
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
                visibility,
                ..
            } => {
                let visibility = if route.chat_id == LEGACY_CHANNEL_DEFAULT_CHAT_ID {
                    RouteAttachmentVisibility::Hidden
                } else {
                    *visibility
                };
                self.current.insert(
                    route.clone(),
                    RouteAttachment {
                        route: route.clone(),
                        workspace_id: workspace_id.clone(),
                        thread_id: thread_id.clone(),
                        attached_at: occurred_at.clone(),
                        visibility,
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

    pub fn is_empty(&self) -> bool {
        self.current.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct RouteAttachmentEventStore {
    path: PathBuf,
    command_tx: mpsc::UnboundedSender<RouteAttachmentStoreCommand>,
}

enum RouteAttachmentStoreCommand {
    Load(oneshot::Sender<jsonl::Result<RouteAttachmentProjection>>),
    Apply(RouteAttachmentEvent, oneshot::Sender<jsonl::Result<()>>),
    Migrate {
        channel_kind: String,
        instance_id: String,
        reply: oneshot::Sender<jsonl::Result<usize>>,
    },
}

impl RouteAttachmentEventStore {
    pub fn default_path() -> PathBuf {
        crate::config::migrate_legacy_state_file("route-attachments.jsonl")
    }

    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        tokio::spawn(run_attachment_store(path.clone(), command_rx));
        Self { path, command_tx }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn append(&self, event: &RouteAttachmentEvent) -> jsonl::Result<()> {
        let (reply, result) = oneshot::channel();
        self.command_tx
            .send(RouteAttachmentStoreCommand::Apply(event.clone(), reply))
            .expect("attachment store owner must outlive its handle");
        result
            .await
            .expect("attachment store owner must reply while its handle is alive")
    }

    #[cfg(test)]
    pub async fn read_events(&self) -> jsonl::Result<Vec<RouteAttachmentEvent>> {
        jsonl::read_all(&self.path).await
    }

    pub async fn load_projection(&self) -> jsonl::Result<RouteAttachmentProjection> {
        let (reply, result) = oneshot::channel();
        self.command_tx
            .send(RouteAttachmentStoreCommand::Load(reply))
            .expect("attachment store owner must outlive its handle");
        result
            .await
            .expect("attachment store owner must reply while its handle is alive")
    }

    pub async fn migrate_channel_instance(
        &self,
        channel_kind: &str,
        instance_id: &str,
    ) -> jsonl::Result<usize> {
        let (reply, result) = oneshot::channel();
        self.command_tx
            .send(RouteAttachmentStoreCommand::Migrate {
                channel_kind: channel_kind.to_string(),
                instance_id: instance_id.to_string(),
                reply,
            })
            .expect("attachment store owner must outlive its handle");
        result
            .await
            .expect("attachment store owner must reply while its handle is alive")
    }
}

async fn run_attachment_store(
    path: PathBuf,
    mut command_rx: mpsc::UnboundedReceiver<RouteAttachmentStoreCommand>,
) {
    let mut projection = None;
    while let Some(command) = command_rx.recv().await {
        let loaded = ensure_attachment_projection_loaded(&path, &mut projection).await;
        match command {
            RouteAttachmentStoreCommand::Load(reply) => {
                let _ = reply.send(
                    loaded.map(|()| projection.as_ref().expect("projection was loaded").clone()),
                );
            }
            RouteAttachmentStoreCommand::Apply(event, reply) => {
                let result = match loaded {
                    Ok(()) => {
                        let mut next = projection.as_ref().expect("projection was loaded").clone();
                        next.apply(&event);
                        let result = replace_attachment_projection(&path, &next).await;
                        if result.is_ok() {
                            projection = Some(next);
                        }
                        result
                    }
                    Err(error) => Err(error),
                };
                let _ = reply.send(result);
            }
            RouteAttachmentStoreCommand::Migrate {
                channel_kind,
                instance_id,
                reply,
            } => {
                let result = match loaded {
                    Ok(()) => {
                        let mut next = projection.as_ref().expect("projection was loaded").clone();
                        let count = migrate_projection(&mut next, &channel_kind, &instance_id);
                        if count == 0 {
                            Ok(0)
                        } else {
                            let result = replace_attachment_projection(&path, &next)
                                .await
                                .map(|()| count);
                            if result.is_ok() {
                                projection = Some(next);
                            }
                            result
                        }
                    }
                    Err(error) => Err(error),
                };
                let _ = reply.send(result);
            }
        }
    }
}

async fn ensure_attachment_projection_loaded(
    path: &Path,
    projection: &mut Option<RouteAttachmentProjection>,
) -> jsonl::Result<()> {
    if projection.is_none() {
        let events = jsonl::read_all(path).await?;
        *projection = Some(RouteAttachmentProjection::from_events(&events));
    }
    Ok(())
}

fn migrate_projection(
    projection: &mut RouteAttachmentProjection,
    channel_kind: &str,
    instance_id: &str,
) -> usize {
    if channel_kind == instance_id {
        return 0;
    }
    let migrations = projection
        .current
        .values()
        .filter(|attachment| {
            attachment.route.channel_kind == channel_kind
                && attachment.route.channel_instance_id() == channel_kind
        })
        .map(|attachment| {
            let route = &attachment.route;
            let migrated_route = RouteKey::with_actor(
                channel_kind,
                instance_id,
                route.chat_id.clone(),
                route.actor_id().unwrap_or(instance_id),
                route.topic_id.clone(),
            );
            (route.clone(), migrated_route, attachment.clone())
        })
        .collect::<Vec<_>>();
    for (legacy_route, migrated_route, attachment) in &migrations {
        projection.current.remove(legacy_route);
        let mut attachment = attachment.clone();
        attachment.route = migrated_route.clone();
        projection
            .current
            .entry(migrated_route.clone())
            .or_insert(attachment);
    }
    migrations.len()
}

async fn replace_attachment_projection(
    path: &Path,
    projection: &RouteAttachmentProjection,
) -> jsonl::Result<()> {
    let events = projection
        .all()
        .map(RouteAttachmentEvent::snapshot)
        .collect::<Vec<_>>();
    jsonl::replace_all(path, &events).await
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

    #[test]
    fn legacy_channel_default_attachment_migrates_to_hidden_visibility() {
        let route = RouteKey::new("slack", LEGACY_CHANNEL_DEFAULT_CHAT_ID);
        let legacy_event = serde_json::json!({
            "event": "attached",
            "schema_version": 1,
            "event_id": "rae_legacy",
            "occurred_at": "2026-01-01T00:00:00Z",
            "route": route,
            "workspace_id": "ws_a",
            "thread_id": "wt_a"
        });
        let event: RouteAttachmentEvent = serde_json::from_value(legacy_event).unwrap();
        let projection = RouteAttachmentProjection::from_events(&[event]);

        assert_eq!(
            projection.all().next().unwrap().visibility,
            RouteAttachmentVisibility::Hidden
        );
    }

    #[tokio::test]
    async fn attachment_writes_replace_superseded_history() {
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

        let events = store.read_events().await.unwrap();

        assert!(events.is_empty());

        let _ = tokio::fs::remove_dir_all(path.parent().unwrap()).await;
    }

    #[tokio::test]
    async fn append_is_visible_in_the_next_projection() {
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

        assert!(store.load_projection().await.unwrap().get(&route).is_some());
        store
            .append(&RouteAttachmentEvent::detached(route.clone()))
            .await
            .unwrap();

        assert!(store.load_projection().await.unwrap().get(&route).is_none());

        let _ = tokio::fs::remove_dir_all(path.parent().unwrap()).await;
    }

    #[tokio::test]
    async fn concurrent_route_writes_are_serialized_without_lost_updates() {
        let path = std::env::temp_dir()
            .join(format!("vibearound-attachments-{}", uuid::Uuid::new_v4()))
            .join("attachments.jsonl");
        let store = RouteAttachmentEventStore::new(path.clone());
        let first = RouteAttachmentEvent::attached(RouteKey::new("web", "a"), "ws", "wt-a");
        let second = RouteAttachmentEvent::attached(RouteKey::new("web", "b"), "ws", "wt-b");

        let (first_result, second_result) =
            tokio::join!(store.append(&first), store.append(&second));
        first_result.unwrap();
        second_result.unwrap();

        let projection = store.load_projection().await.unwrap();
        assert_eq!(projection.len(), 2);

        let _ = tokio::fs::remove_dir_all(path.parent().unwrap()).await;
    }

    #[tokio::test]
    async fn channel_instance_migration_rewrites_legacy_route_once() {
        let path = std::env::temp_dir()
            .join(format!("vibearound-attachments-{}", uuid::Uuid::new_v4()))
            .join("attachments.jsonl");
        let store = RouteAttachmentEventStore::new(path.clone());
        let legacy_route = RouteKey::new("slack", "chat-a");
        store
            .append(&RouteAttachmentEvent::attached(
                legacy_route.clone(),
                "ws_a",
                "wt_a",
            ))
            .await
            .unwrap();

        assert_eq!(
            store
                .migrate_channel_instance("slack", "slack-primary")
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            store
                .migrate_channel_instance("slack", "slack-primary")
                .await
                .unwrap(),
            0
        );
        let projection = store.load_projection().await.unwrap();
        let migrated =
            RouteKey::with_actor("slack", "slack-primary", "chat-a", "slack-primary", None);
        assert!(projection.get(&legacy_route).is_none());
        assert_eq!(
            projection.get(&migrated).unwrap().thread_id,
            WorkspaceThreadId::from("wt_a")
        );

        let _ = tokio::fs::remove_dir_all(path.parent().unwrap()).await;
    }
}
