//! Workspace domain model.
//!
//! A workspace owns threads. IM/Web routes attach to workspace threads; agent
//! sessions remain implementation details of each thread runtime.

pub mod handover;
pub mod manager;
pub mod registry;
mod runtime_registry;
pub mod store;
pub mod threads;

use std::path::PathBuf;

pub use manager::{normalize_workspace_cwd, WorkspaceThreadManager};
pub use registry::{WorkspaceId, WorkspaceProjection, WorkspaceRecord, GENERAL_WORKSPACE_ID};

#[cfg(windows)]
pub(crate) fn normalize_platform_cwd(path: PathBuf) -> PathBuf {
    let value = path.to_string_lossy();
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = value.strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }
    path
}

#[cfg(not(windows))]
pub(crate) fn normalize_platform_cwd(path: PathBuf) -> PathBuf {
    path
}
