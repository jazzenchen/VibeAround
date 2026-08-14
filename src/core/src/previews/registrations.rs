//! Durable projection of File Preview registrations.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::store::{canonical, slug_from_path, PreviewSession};
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
    // Older versions persisted Server previews. Accept and discard those
    // records so an upgrade still restores File previews from the same file.
    Server {},
}

pub(super) fn activate_path() -> &'static Path {
    ACTIVE_PATH
        .get_or_init(|| {
            let path = crate::config::state_file(FILE_NAME);
            let legacy_path = crate::config::state_file(LEGACY_FILE_NAME);
            if let Err(error) = remove_legacy_at(&legacy_path) {
                tracing::warn!(
                    path = ?legacy_path,
                    error = %error,
                    "failed to remove obsolete Server preview leases"
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
        let PreviewRegistration::File {
            file,
            workspace,
            title,
        } = registration
        else {
            continue;
        };
        let file = canonical(&file);
        let workspace = canonical(&workspace);
        if !valid_file_registration(&file, &workspace) || sessions.contains_key(&file) {
            continue;
        }
        sessions.insert(
            file.clone(),
            PreviewSession {
                id: file.clone(),
                workspace,
                title,
                target: PreviewTarget::File,
                slug: slug_from_path(&file),
                share: None,
                created_at: Instant::now(),
            },
        );
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
            PreviewTarget::Server { .. } => None,
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
        _ => std::cmp::Ordering::Equal,
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

fn remove_legacy_at(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("remove {}", path.display())),
    }
}

#[cfg(test)]
mod tests;
