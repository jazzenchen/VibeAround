//! Durable projection of live Server preview registrations.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::store::{canonical, slug_from_path, ListenerProcess, PreviewSession};
use super::PreviewTarget;

const FILE_NAME: &str = "preview-server-leases.json";

// Persistence is activated during daemon startup. Unit tests that exercise
// the in-memory API directly therefore never touch the user's state dir.
static ACTIVE_PATH: OnceLock<PathBuf> = OnceLock::new();

#[derive(Debug, Serialize, Deserialize)]
struct ServerLease {
    workspace: PathBuf,
    port: u16,
    title: String,
    listener: ListenerProcess,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner_session: Option<String>,
}

pub(super) fn activate_path() -> &'static Path {
    ACTIVE_PATH
        .get_or_init(|| crate::config::state_file(FILE_NAME))
        .as_path()
}

pub(super) fn persist_active(sessions: &HashMap<PathBuf, PreviewSession>) {
    let Some(path) = ACTIVE_PATH.get() else {
        return;
    };
    if let Err(error) = persist_at(path, sessions) {
        tracing::warn!(path = ?path, error = %error, "failed to persist Server preview leases");
    }
}

pub(super) fn reconcile_at(
    path: &Path,
    sessions: &mut HashMap<PathBuf, PreviewSession>,
    mut listener_at: impl FnMut(u16) -> Option<ListenerProcess>,
) -> Result<usize> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            persist_at(path, sessions)?;
            return Ok(0);
        }
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    let leases: Vec<ServerLease> =
        serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))?;

    let mut restored = 0;
    for lease in leases {
        if listener_at(lease.port) != Some(lease.listener) {
            continue;
        }

        let workspace = canonical(&lease.workspace);
        let id = workspace.join(format!(":port:{}", lease.port));
        if sessions.contains_key(&id) {
            continue;
        }
        sessions.insert(
            id.clone(),
            PreviewSession {
                id: id.clone(),
                workspace,
                title: lease.title,
                target: PreviewTarget::Server { port: lease.port },
                listener: Some(lease.listener),
                slug: slug_from_path(&id),
                share: None,
                conversation_thread_id: None,
                owner_session: lease.owner_session,
                created_at: Instant::now(),
            },
        );
        restored += 1;
    }

    persist_at(path, sessions)?;
    Ok(restored)
}

pub(super) fn persist_at(path: &Path, sessions: &HashMap<PathBuf, PreviewSession>) -> Result<()> {
    let mut leases = sessions
        .values()
        .filter_map(|session| match (&session.target, session.listener) {
            (PreviewTarget::Server { port }, Some(listener)) => Some(ServerLease {
                workspace: canonical(&session.workspace),
                port: *port,
                title: session.title.clone(),
                listener,
                owner_session: session.owner_session.clone(),
            }),
            _ => None,
        })
        .collect::<Vec<_>>();
    leases.sort_by(|left, right| {
        left.workspace
            .cmp(&right.workspace)
            .then(left.port.cmp(&right.port))
    });

    if leases.is_empty() {
        return match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).with_context(|| format!("remove {}", path.display())),
        };
    }

    let contents = serde_json::to_vec_pretty(&leases)?;
    crate::file_replace::write_private(path, contents)
}

#[cfg(test)]
mod tests;
