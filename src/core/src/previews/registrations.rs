//! Durable projection of Preview registrations.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::store::{canonical, slug_from_path, ListenerProcess, PreviewSession};
use super::PreviewTarget;

const FILE_NAME: &str = "preview-registrations.json";
const LEGACY_FILE_NAME: &str = "preview-server-leases.json";

// Persistence is activated during daemon startup. Unit tests that exercise
// the in-memory API directly therefore never touch the user's state dir.
static ACTIVE_PATH: OnceLock<PathBuf> = OnceLock::new();

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PreviewRegistration {
    File {
        file: PathBuf,
        workspace: PathBuf,
        title: String,
    },
    Server {
        workspace: PathBuf,
        port: u16,
        title: String,
        listener: ListenerProcess,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner_session: Option<String>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct LegacyServerLease {
    workspace: PathBuf,
    port: u16,
    title: String,
    listener: ListenerProcess,
    #[serde(default)]
    owner_session: Option<String>,
}

pub(super) fn activate_path() -> &'static Path {
    ACTIVE_PATH
        .get_or_init(|| {
            let path = crate::config::state_file(FILE_NAME);
            let legacy_path = crate::config::state_file(LEGACY_FILE_NAME);
            if let Err(error) = migrate_legacy_at(&legacy_path, &path) {
                tracing::warn!(
                    path = ?legacy_path,
                    error = %error,
                    "failed to migrate legacy Server preview leases"
                );
            }
            path
        })
        .as_path()
}

pub(super) fn persist_active(sessions: &HashMap<PathBuf, PreviewSession>) {
    let Some(path) = ACTIVE_PATH.get() else {
        return;
    };
    if let Err(error) = persist_at(path, sessions) {
        tracing::warn!(path = ?path, error = %error, "failed to persist Preview registrations");
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
    let registrations: Vec<PreviewRegistration> =
        serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))?;

    let mut restored = 0;
    for registration in registrations {
        let session = match registration {
            PreviewRegistration::File {
                file,
                workspace,
                title,
            } => {
                let file = canonical(&file);
                let workspace = canonical(&workspace);
                if !valid_file_registration(&file, &workspace) {
                    continue;
                }
                PreviewSession {
                    id: file.clone(),
                    workspace,
                    title,
                    target: PreviewTarget::File,
                    listener: None,
                    slug: slug_from_path(&file),
                    share: None,
                    conversation_thread_id: None,
                    owner_session: None,
                    created_at: Instant::now(),
                }
            }
            PreviewRegistration::Server {
                workspace,
                port,
                title,
                listener,
                owner_session,
            } => {
                if listener_at(port) != Some(listener) {
                    continue;
                }
                let workspace = canonical(&workspace);
                let id = workspace.join(format!(":port:{port}"));
                PreviewSession {
                    id: id.clone(),
                    workspace,
                    title,
                    target: PreviewTarget::Server { port },
                    listener: Some(listener),
                    slug: slug_from_path(&id),
                    share: None,
                    conversation_thread_id: None,
                    owner_session,
                    created_at: Instant::now(),
                }
            }
        };
        if sessions.contains_key(&session.id) {
            continue;
        }
        sessions.insert(session.id.clone(), session);
        restored += 1;
    }

    persist_at(path, sessions)?;
    Ok(restored)
}

pub(super) fn persist_at(path: &Path, sessions: &HashMap<PathBuf, PreviewSession>) -> Result<()> {
    let registrations = sessions
        .values()
        .filter_map(|session| match &session.target {
            PreviewTarget::File => {
                let file = canonical(&session.id);
                let workspace = canonical(&session.workspace);
                valid_file_registration(&file, &workspace).then(|| PreviewRegistration::File {
                    file,
                    workspace,
                    title: session.title.clone(),
                })
            }
            PreviewTarget::Server { port } => {
                session
                    .listener
                    .map(|listener| PreviewRegistration::Server {
                        workspace: canonical(&session.workspace),
                        port: *port,
                        title: session.title.clone(),
                        listener,
                        owner_session: session.owner_session.clone(),
                    })
            }
        })
        .collect::<Vec<_>>();
    persist_registrations_at(path, registrations)
}

fn valid_file_registration(file: &Path, workspace: &Path) -> bool {
    file.is_file() && workspace.is_dir()
}

fn persist_registrations_at(
    path: &Path,
    mut registrations: Vec<PreviewRegistration>,
) -> Result<()> {
    registrations.sort_by(|left, right| match (left, right) {
        (
            PreviewRegistration::File { file: left, .. },
            PreviewRegistration::File { file: right, .. },
        ) => left.cmp(right),
        (PreviewRegistration::File { .. }, PreviewRegistration::Server { .. }) => {
            std::cmp::Ordering::Less
        }
        (PreviewRegistration::Server { .. }, PreviewRegistration::File { .. }) => {
            std::cmp::Ordering::Greater
        }
        (
            PreviewRegistration::Server {
                workspace: left_workspace,
                port: left_port,
                ..
            },
            PreviewRegistration::Server {
                workspace: right_workspace,
                port: right_port,
                ..
            },
        ) => left_workspace
            .cmp(right_workspace)
            .then(left_port.cmp(right_port)),
    });

    if registrations.is_empty() {
        return match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).with_context(|| format!("remove {}", path.display())),
        };
    }

    let contents = serde_json::to_vec_pretty(&registrations)?;
    crate::file_replace::write_private(path, contents)
}

fn migrate_legacy_at(legacy_path: &Path, path: &Path) -> Result<()> {
    if !legacy_path.exists() {
        return Ok(());
    }
    if path.exists() {
        return std::fs::remove_file(legacy_path)
            .with_context(|| format!("remove {}", legacy_path.display()));
    }
    let bytes =
        std::fs::read(legacy_path).with_context(|| format!("read {}", legacy_path.display()))?;
    let leases: Vec<LegacyServerLease> = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse {}", legacy_path.display()))?;
    let registrations = leases
        .into_iter()
        .map(|lease| PreviewRegistration::Server {
            workspace: lease.workspace,
            port: lease.port,
            title: lease.title,
            listener: lease.listener,
            owner_session: lease.owner_session,
        })
        .collect();
    persist_registrations_at(path, registrations)?;
    std::fs::remove_file(legacy_path).with_context(|| format!("remove {}", legacy_path.display()))
}

#[cfg(test)]
mod tests;
