//! Append-only JSONL event store helpers.

use std::path::{Path, PathBuf};

use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Debug, Error)]
pub enum JsonlError {
    #[error("jsonl io error at {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to serialize jsonl event: {0}")]
    Serialize(#[source] serde_json::Error),
    #[error("failed to parse jsonl event at {path:?}:{line}: {source}")]
    Deserialize {
        path: PathBuf,
        line: usize,
        #[source]
        source: serde_json::Error,
    },
}

pub type Result<T> = std::result::Result<T, JsonlError>;

/// Append one JSON value as a single JSONL record.
pub async fn append<T>(path: impl AsRef<Path>, event: &T) -> Result<()>
where
    T: Serialize + ?Sized,
{
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|source| JsonlError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
    }

    let mut line = serde_json::to_vec(event).map_err(JsonlError::Serialize)?;
    line.push(b'\n');

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
        .map_err(|source| JsonlError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    file.write_all(&line)
        .await
        .map_err(|source| JsonlError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    file.flush().await.map_err(|source| JsonlError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    crate::auth::set_owner_only(path).map_err(|source| JsonlError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    Ok(())
}

/// Replace a JSONL store with exactly these records.
pub async fn replace_all<T>(path: impl AsRef<Path>, events: &[T]) -> Result<()>
where
    T: Serialize,
{
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|source| JsonlError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
    }

    let tmp = path.with_extension(format!("jsonl.tmp-{}", uuid::Uuid::new_v4().simple()));
    let mut file = tokio::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&tmp)
        .await
        .map_err(|source| JsonlError::Io {
            path: tmp.clone(),
            source,
        })?;
    for event in events {
        let mut line = serde_json::to_vec(event).map_err(JsonlError::Serialize)?;
        line.push(b'\n');
        file.write_all(&line)
            .await
            .map_err(|source| JsonlError::Io {
                path: tmp.clone(),
                source,
            })?;
    }
    file.flush().await.map_err(|source| JsonlError::Io {
        path: tmp.clone(),
        source,
    })?;
    drop(file);

    crate::auth::set_owner_only(&tmp).map_err(|source| JsonlError::Io {
        path: tmp.clone(),
        source,
    })?;
    tokio::fs::rename(&tmp, path)
        .await
        .map_err(|source| JsonlError::Io {
            path: path.to_path_buf(),
            source,
        })?;

    Ok(())
}

/// Read every non-empty JSONL record in order.
pub async fn read_all<T>(path: impl AsRef<Path>) -> Result<Vec<T>>
where
    T: DeserializeOwned,
{
    let path = path.as_ref();
    let file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(JsonlError::Io {
                path: path.to_path_buf(),
                source,
            })
        }
    };

    let mut lines = BufReader::new(file).lines();
    let mut out = Vec::new();
    let mut line_no = 0usize;
    while let Some(line) = lines.next_line().await.map_err(|source| JsonlError::Io {
        path: path.to_path_buf(),
        source,
    })? {
        line_no += 1;
        if line.trim().is_empty() {
            continue;
        }
        let event = serde_json::from_str(&line).map_err(|source| JsonlError::Deserialize {
            path: path.to_path_buf(),
            line: line_no,
            source,
        })?;
        out.push(event);
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;

    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct TestEvent {
        id: String,
        value: i32,
    }

    fn temp_jsonl_path(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("vibearound-jsonl-{}-{}", name, Uuid::new_v4()))
            .join("events.jsonl")
    }

    #[tokio::test]
    async fn append_and_read_events_in_order() {
        let path = temp_jsonl_path("ordered");

        append(
            &path,
            &TestEvent {
                id: "a".to_string(),
                value: 1,
            },
        )
        .await
        .unwrap();
        append(
            &path,
            &TestEvent {
                id: "b".to_string(),
                value: 2,
            },
        )
        .await
        .unwrap();

        let events: Vec<TestEvent> = read_all(&path).await.unwrap();
        assert_eq!(
            events,
            vec![
                TestEvent {
                    id: "a".to_string(),
                    value: 1,
                },
                TestEvent {
                    id: "b".to_string(),
                    value: 2,
                }
            ]
        );

        let _ = tokio::fs::remove_dir_all(path.parent().unwrap()).await;
    }

    #[tokio::test]
    async fn missing_file_reads_as_empty_log() {
        let path = temp_jsonl_path("missing");

        let events: Vec<TestEvent> = read_all(&path).await.unwrap();

        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn replace_all_overwrites_existing_events() {
        let path = temp_jsonl_path("replace");
        append(
            &path,
            &TestEvent {
                id: "old".to_string(),
                value: 1,
            },
        )
        .await
        .unwrap();

        replace_all(
            &path,
            &[TestEvent {
                id: "new".to_string(),
                value: 2,
            }],
        )
        .await
        .unwrap();

        let events: Vec<TestEvent> = read_all(&path).await.unwrap();
        assert_eq!(
            events,
            vec![TestEvent {
                id: "new".to_string(),
                value: 2,
            }]
        );

        let _ = tokio::fs::remove_dir_all(path.parent().unwrap()).await;
    }

    #[tokio::test]
    async fn parse_error_reports_line_number() {
        let path = temp_jsonl_path("bad-line");
        tokio::fs::create_dir_all(path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&path, "{\"id\":\"ok\",\"value\":1}\nnot json\n")
            .await
            .unwrap();

        let error = read_all::<TestEvent>(&path).await.unwrap_err();

        assert!(matches!(error, JsonlError::Deserialize { line: 2, .. }));

        let _ = tokio::fs::remove_dir_all(path.parent().unwrap()).await;
    }
}
