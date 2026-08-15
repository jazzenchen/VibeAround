//! Cleanup-only journal of Server Preview ports.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result};

const FILE_NAME: &str = "preview-cleanup-ports.json";

// Activated by daemon startup so library tests do not write user state.
static ACTIVE_PATH: OnceLock<PathBuf> = OnceLock::new();

pub(super) fn activate_path() -> &'static Path {
    ACTIVE_PATH
        .get_or_init(|| crate::config::state_file(FILE_NAME))
        .as_path()
}

pub(super) fn persist_active(ports: &[u16]) {
    let Some(path) = ACTIVE_PATH.get() else {
        return;
    };
    if let Err(error) = persist_at(path, ports) {
        tracing::warn!(path = ?path, error = %error, "failed to persist Preview cleanup ports");
    }
}

pub(super) fn read_at(path: &Path) -> Result<Vec<u16>> {
    match std::fs::read(path) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
    }
}

pub(super) fn remove_at(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("remove {}", path.display())),
    }
}

fn persist_at(path: &Path, ports: &[u16]) -> Result<()> {
    let mut ports = ports.to_vec();
    ports.sort_unstable();
    ports.dedup();
    if ports.is_empty() {
        return remove_at(path);
    }
    crate::file_replace::write_private(path, serde_json::to_vec(&ports)?)
}

#[cfg(test)]
mod tests;
