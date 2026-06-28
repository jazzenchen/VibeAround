//! Terminal launcher planning.
//!
//! This module owns profile/direct launch planning. Concern-specific modules
//! render profile/bridge/Codex details; platform modules execute the final plan
//! in the user's selected terminal.

mod bridge;
mod claude_desktop;
mod codex;
mod codex_desktop;
mod common;
mod plan;
mod va_launch;

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
    spawn_plan(
        plan,
        va_launch::LaunchContext::profile(profile, launch_target, None),
    )
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
    spawn_plan(
        plan,
        va_launch::LaunchContext::profile(profile, launch_target, Some(session_id)),
    )
}

/// "Direct" launch opens the named coding CLI without profile credential env.
/// The CLI uses whatever global OAuth/login/config it already has on disk.
pub fn launch_direct(agent_id: &str) -> anyhow::Result<()> {
    let plan = LaunchPlanBuilder::new().direct(agent_id).build()?;
    spawn_plan(plan, va_launch::LaunchContext::direct(agent_id, None))
}

pub fn launch_direct_resume(agent_id: &str, session_id: &str) -> anyhow::Result<()> {
    let plan = LaunchPlanBuilder::new()
        .direct(agent_id)
        .resume(session_id)
        .build()?;
    spawn_plan(
        plan,
        va_launch::LaunchContext::direct(agent_id, Some(session_id)),
    )
}

fn spawn_plan(
    launch_plan: common::LaunchPlan,
    context: va_launch::LaunchContext,
) -> anyhow::Result<()> {
    va_launch::spawn(&launch_plan, &context)
}
