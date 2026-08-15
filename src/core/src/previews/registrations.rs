//! Cleanup-only journal for Preview registrations.
//!
//! The journal is never used to recreate a Preview. It only records enough
//! information for the next daemon run to repeat cleanup after an interrupted
//! shutdown.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::store::PreviewSession;
use super::PreviewTarget;

const FILE_NAME: &str = "preview-registrations.json";
const LEGACY_FILE_NAME: &str = "preview-server-leases.json";

// Persistence is activated by the daemon's startup cleanup. Unit tests that
// exercise the in-memory Preview API directly therefore never touch user data.
static ACTIVE_PATHS: OnceLock<RegistrationPaths> = OnceLock::new();

struct RegistrationPaths {
    current: PathBuf,
    legacy: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CleanupRegistration {
    File {},
    Server { port: u16 },
}

#[derive(Debug, Deserialize)]
struct LegacyServerRegistration {
    port: u16,
}

pub(super) fn activate_paths() -> (&'static Path, &'static Path) {
    let paths = ACTIVE_PATHS.get_or_init(|| RegistrationPaths {
        current: crate::config::state_file(FILE_NAME),
        legacy: crate::config::state_file(LEGACY_FILE_NAME),
    });
    (&paths.current, &paths.legacy)
}

pub(super) fn persist_active(sessions: &HashMap<PathBuf, PreviewSession>) {
    let Some(paths) = ACTIVE_PATHS.get() else {
        return;
    };
    if let Err(error) = persist_at(&paths.current, sessions) {
        tracing::warn!(
            path = ?paths.current,
            error = %error,
            "failed to persist Preview cleanup registrations"
        );
    }
}

pub(super) fn current_server_ports_at(path: &Path) -> Result<Vec<u16>> {
    let Some(bytes) = read_optional(path)? else {
        return Ok(Vec::new());
    };
    let registrations: Vec<CleanupRegistration> =
        serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))?;
    Ok(registrations
        .into_iter()
        .filter_map(|registration| match registration {
            CleanupRegistration::File {} => None,
            CleanupRegistration::Server { port } => Some(port),
        })
        .collect())
}

pub(super) fn legacy_server_ports_at(path: &Path) -> Result<Vec<u16>> {
    let Some(bytes) = read_optional(path)? else {
        return Ok(Vec::new());
    };
    let registrations: Vec<LegacyServerRegistration> =
        serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))?;
    Ok(registrations
        .into_iter()
        .map(|registration| registration.port)
        .collect())
}

pub(super) fn remove_at(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("remove {}", path.display())),
    }
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
    }
}

fn persist_at(path: &Path, sessions: &HashMap<PathBuf, PreviewSession>) -> Result<()> {
    let registrations = sessions
        .values()
        .map(|session| match session.target {
            PreviewTarget::File => CleanupRegistration::File {},
            PreviewTarget::Server { port } => CleanupRegistration::Server { port },
        })
        .collect::<Vec<_>>();

    if registrations.is_empty() {
        return remove_at(path);
    }

    let contents = serde_json::to_vec_pretty(&registrations)?;
    crate::file_replace::write_private(path, contents)
}

#[cfg(test)]
mod tests;
