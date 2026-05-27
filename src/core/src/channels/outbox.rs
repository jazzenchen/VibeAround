//! JSONL-backed channel output outbox.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

use crate::routing::ChannelKind;
use crate::storage::jsonl;

use super::ChannelOutput;

const SCHEMA_VERSION: u8 = 1;
const MAX_REPLAY_BYTES: u64 = 64 * 1024 * 1024;
const INTERNAL_WEB_CHANNEL: &str = "web";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum OutboxEvent {
    Enqueued {
        schema_version: u8,
        output_id: String,
        channel_kind: ChannelKind,
        output: ChannelOutput,
    },
    Sent {
        schema_version: u8,
        output_id: String,
    },
    Acked {
        schema_version: u8,
        output_id: String,
    },
    Nacked {
        schema_version: u8,
        output_id: String,
        reason: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct PendingOutput {
    pub output_id: String,
    pub output: ChannelOutput,
}

pub struct ChannelOutbox {
    path: PathBuf,
    pending: DashMap<String, ChannelOutput>,
}

impl ChannelOutbox {
    pub fn new_default() -> Self {
        Self::new(crate::config::migrate_legacy_state_file(
            "channel-outbox.jsonl",
        ))
    }

    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let pending = hydrate_pending(&path);
        if let Err(error) = compact_pending(&path, &pending) {
            tracing::warn!(path = ?path, error = %error, "failed to compact channel outbox");
        }
        Self { path, pending }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn enqueue(&self, output: ChannelOutput) -> jsonl::Result<String> {
        let output_id = format!("out_{}", uuid::Uuid::new_v4().simple());
        let channel_kind = output.route_key().channel_kind.clone();
        jsonl::append(
            &self.path,
            &OutboxEvent::Enqueued {
                schema_version: SCHEMA_VERSION,
                output_id: output_id.clone(),
                channel_kind,
                output: output.clone(),
            },
        )
        .await?;
        self.pending.insert(output_id.clone(), output);
        Ok(output_id)
    }

    pub async fn mark_sent(&self, output_id: &str) -> jsonl::Result<()> {
        jsonl::append(
            &self.path,
            &OutboxEvent::Sent {
                schema_version: SCHEMA_VERSION,
                output_id: output_id.to_string(),
            },
        )
        .await?;
        self.pending.remove(output_id);
        Ok(())
    }

    pub async fn mark_nacked(&self, output_id: &str, reason: Option<String>) -> jsonl::Result<()> {
        jsonl::append(
            &self.path,
            &OutboxEvent::Nacked {
                schema_version: SCHEMA_VERSION,
                output_id: output_id.to_string(),
                reason,
            },
        )
        .await?;
        self.pending.remove(output_id);
        Ok(())
    }

    pub fn pending_for_channel(&self, channel_kind: &str) -> Vec<PendingOutput> {
        self.pending
            .iter()
            .filter(|entry| entry.value().route_key().channel_kind == channel_kind)
            .map(|entry| PendingOutput {
                output_id: entry.key().clone(),
                output: entry.value().clone(),
            })
            .collect()
    }
}

fn hydrate_pending(path: &Path) -> DashMap<String, ChannelOutput> {
    let pending = DashMap::new();
    if let Ok(metadata) = fs::metadata(path) {
        if metadata.len() > MAX_REPLAY_BYTES {
            match crate::config::archive_state_file(path, "oversized-outbox") {
                Ok(archive) => {
                    tracing::warn!(
                        path = ?path,
                        archive = ?archive,
                        bytes = metadata.len(),
                        "archived oversized channel outbox instead of replaying it"
                    );
                }
                Err(error) => {
                    tracing::warn!(
                        path = ?path,
                        bytes = metadata.len(),
                        error = %error,
                        "failed to archive oversized channel outbox"
                    );
                }
            }
            return pending;
        }
    }
    let Ok(file) = std::fs::File::open(path) else {
        return pending;
    };
    let reader = std::io::BufReader::new(file);
    use std::io::BufRead;
    for line in reader.lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<OutboxEvent>(trimmed) else {
            continue;
        };
        match event {
            OutboxEvent::Enqueued {
                output_id,
                channel_kind,
                output,
                ..
            } => {
                if channel_kind == INTERNAL_WEB_CHANNEL
                    || output.route_key().channel_kind == INTERNAL_WEB_CHANNEL
                {
                    continue;
                }
                pending.insert(output_id, output);
            }
            OutboxEvent::Sent { output_id, .. }
            | OutboxEvent::Acked { output_id, .. }
            | OutboxEvent::Nacked { output_id, .. } => {
                pending.remove(&output_id);
            }
        }
    }
    pending
}

fn compact_pending(path: &Path, pending: &DashMap<String, ChannelOutput>) -> std::io::Result<()> {
    if pending.is_empty() {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("jsonl.tmp-{}", uuid::Uuid::new_v4().simple()));
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&tmp)?;
    for entry in pending.iter() {
        let output = entry.value();
        let event = OutboxEvent::Enqueued {
            schema_version: SCHEMA_VERSION,
            output_id: entry.key().clone(),
            channel_kind: output.route_key().channel_kind.clone(),
            output: output.clone(),
        };
        serde_json::to_writer(&mut file, &event)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
        file.write_all(b"\n")?;
    }
    file.flush()?;
    crate::auth::set_owner_only(&tmp)?;
    fs::rename(tmp, path)?;
    crate::auth::set_owner_only(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::routing::RouteKey;

    use super::*;

    #[tokio::test]
    async fn pending_outputs_filter_by_channel() {
        let path = std::env::temp_dir()
            .join(format!("vibearound-outbox-{}", uuid::Uuid::new_v4()))
            .join("outbox.jsonl");
        let outbox = ChannelOutbox::new(path.clone());
        let feishu = ChannelOutput::SystemText {
            route: RouteKey::new("feishu", "chat-a"),
            text: "hello".to_string(),
            reply_to: None,
        };
        let web = ChannelOutput::SystemText {
            route: RouteKey::new("web", "chat-a"),
            text: "hello".to_string(),
            reply_to: None,
        };

        let feishu_id = outbox.enqueue(feishu).await.unwrap();
        let _ = outbox.enqueue(web).await.unwrap();

        let pending = outbox.pending_for_channel("feishu");

        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].output_id, feishu_id);

        let _ = tokio::fs::remove_dir_all(path.parent().unwrap()).await;
    }

    #[tokio::test]
    async fn new_hydrates_unsent_outputs_from_jsonl() {
        let path = std::env::temp_dir()
            .join(format!("vibearound-outbox-{}", uuid::Uuid::new_v4()))
            .join("outbox.jsonl");
        let outbox = ChannelOutbox::new(path.clone());
        let output = ChannelOutput::SystemText {
            route: RouteKey::new("feishu", "chat-a"),
            text: "hello".to_string(),
            reply_to: None,
        };
        let output_id = outbox.enqueue(output).await.unwrap();

        let reloaded = ChannelOutbox::new(path.clone());
        let pending = reloaded.pending_for_channel("feishu");

        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].output_id, output_id);

        let _ = tokio::fs::remove_dir_all(path.parent().unwrap()).await;
    }

    #[tokio::test]
    async fn new_ignores_sent_outputs_from_jsonl() {
        let path = std::env::temp_dir()
            .join(format!("vibearound-outbox-{}", uuid::Uuid::new_v4()))
            .join("outbox.jsonl");
        let outbox = ChannelOutbox::new(path.clone());
        let output = ChannelOutput::SystemText {
            route: RouteKey::new("feishu", "chat-a"),
            text: "hello".to_string(),
            reply_to: None,
        };
        let output_id = outbox.enqueue(output).await.unwrap();
        outbox.mark_sent(&output_id).await.unwrap();

        let reloaded = ChannelOutbox::new(path.clone());

        assert!(reloaded.pending_for_channel("feishu").is_empty());

        let _ = tokio::fs::remove_dir_all(path.parent().unwrap()).await;
    }

    #[tokio::test]
    async fn new_compacts_sent_outputs_on_load() {
        let path = std::env::temp_dir()
            .join(format!("vibearound-outbox-{}", uuid::Uuid::new_v4()))
            .join("outbox.jsonl");
        let outbox = ChannelOutbox::new(path.clone());
        let output = ChannelOutput::SystemText {
            route: RouteKey::new("feishu", "chat-a"),
            text: "hello".to_string(),
            reply_to: None,
        };
        let output_id = outbox.enqueue(output).await.unwrap();
        outbox.mark_sent(&output_id).await.unwrap();

        let reloaded = ChannelOutbox::new(path.clone());

        assert!(reloaded.pending_for_channel("feishu").is_empty());
        assert_eq!(
            tokio::fs::read_to_string(&path).await.unwrap_or_default(),
            ""
        );

        let _ = tokio::fs::remove_dir_all(path.parent().unwrap()).await;
    }

    #[tokio::test]
    async fn new_drops_web_outputs_on_load() {
        let path = std::env::temp_dir()
            .join(format!("vibearound-outbox-{}", uuid::Uuid::new_v4()))
            .join("outbox.jsonl");
        let outbox = ChannelOutbox::new(path.clone());
        let output = ChannelOutput::SystemText {
            route: RouteKey::new("web", "chat-a"),
            text: "hello".to_string(),
            reply_to: None,
        };
        let _ = outbox.enqueue(output).await.unwrap();

        let reloaded = ChannelOutbox::new(path.clone());

        assert!(reloaded.pending_for_channel("web").is_empty());
        assert_eq!(
            tokio::fs::read_to_string(&path).await.unwrap_or_default(),
            ""
        );

        let _ = tokio::fs::remove_dir_all(path.parent().unwrap()).await;
    }
}
