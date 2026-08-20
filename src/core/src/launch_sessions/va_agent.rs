use std::path::Path;

use crate::config;

use super::{pi, LaunchSession};

pub(super) fn sessions(workspace: &Path) -> Vec<LaunchSession> {
    let root = config::data_dir()
        .join("agents")
        .join("va-agent")
        .join("sessions");
    pi::sessions_from_root(&root, workspace, "va-agent", "va-agent")
}
