//! Terminal launcher planning.
//!
//! This module owns profile/direct launch planning. Concern-specific modules
//! render profile/bridge/Codex details; platform modules execute the final plan
//! in the user's selected terminal.

mod bridge;
mod codex;
mod common;
mod plan;

#[cfg(target_os = "macos")]
#[path = "launcher/macos.rs"]
mod platform;
#[cfg(target_os = "windows")]
#[path = "launcher/windows.rs"]
mod platform;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[path = "launcher/linux.rs"]
mod platform;

#[cfg(all(test, target_os = "macos"))]
#[allow(dead_code)]
#[path = "launcher/linux.rs"]
mod linux_for_tests;
#[cfg(all(test, not(target_os = "windows")))]
#[allow(dead_code)]
#[path = "launcher/windows.rs"]
mod windows_for_tests;

use self::plan::LaunchPlanBuilder;
use ::common::profiles;

use profiles::ProfileDef;

// ---------------------------------------------------------------------------
// Public entry
// ---------------------------------------------------------------------------

pub fn launch(profile: &ProfileDef, launch_target: &str) -> anyhow::Result<()> {
    let plan = LaunchPlanBuilder::new()
        .profile(profile, launch_target)
        .build()?;
    platform::spawn(plan)
}

pub fn launch_resume(
    profile: &ProfileDef,
    launch_target: &str,
    session_id: &str,
) -> anyhow::Result<()> {
    let plan = LaunchPlanBuilder::new()
        .profile(profile, launch_target)
        .resume(session_id)
        .build()?;
    platform::spawn(plan)
}

/// "Direct" launch opens the named coding CLI with no env injection. The CLI
/// uses whatever global OAuth/login/config it already has on disk.
pub fn launch_direct(agent_id: &str) -> anyhow::Result<()> {
    let plan = LaunchPlanBuilder::new().direct(agent_id).build()?;
    platform::spawn(plan)
}

pub fn launch_direct_resume(agent_id: &str, session_id: &str) -> anyhow::Result<()> {
    let plan = LaunchPlanBuilder::new()
        .direct(agent_id)
        .resume(session_id)
        .build()?;
    platform::spawn(plan)
}
