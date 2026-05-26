//! Skill file install/uninstall.
//!
//! Each agent gets the common VibeAround skills (`vibearound`, `va-session`,
//! `va-preview`, `va-md-preview`); selected agents can receive additional
//! skills while their workflows are being validated.
//!
//! The `include_str!` paths are relative to this source file: `src/core/
//! src/agent/skills.rs` → `../../../skills/...` reaches the top-level
//! `src/skills/` directory where the skill markdown lives.

use anyhow::Context;

use crate::resources;

use super::mcp::home_dir;

/// Install all skill files for a given agent.
pub(super) fn install_skill(agent: &str) -> anyhow::Result<()> {
    let agent_def = match resources::agent_by_id(agent) {
        Some(def) => def,
        None => return Ok(()),
    };
    let global_config = match &agent_def.global_config {
        Some(cfg) => cfg,
        None => return Ok(()),
    };
    let skill_dir_rel = match &global_config.skill_dir {
        Some(dir) => dir,
        None => return Ok(()),
    };

    let home = home_dir()?;
    let primary_skill_dir = home.join(skill_dir_rel);

    // Derive the parent directory for skill deployment.
    // e.g. ".claude/skills/vibearound" → ".claude/skills"
    // For agents with skill_filename (shared dirs like .cursor/rules/),
    // the skill_dir IS the parent.
    let has_skill_filename = global_config.skill_filename.is_some();
    let skill_base = if has_skill_filename {
        primary_skill_dir.clone()
    } else {
        primary_skill_dir
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or(primary_skill_dir.clone())
    };

    for (skill_name, content) in agent_skills(agent) {
        if has_skill_filename {
            // Shared directory (e.g. .cursor/rules/) — use skill-specific filename
            let ext = global_config
                .skill_filename
                .as_deref()
                .and_then(|f| f.rsplit('.').next())
                .unwrap_or("md");
            let filename = format!("{}.{}", skill_name, ext);
            let target = skill_base.join(&filename);
            let _ = std::fs::create_dir_all(&skill_base);
            std::fs::write(&target, content).with_context(|| format!("Write {:?}", target))?;
            tracing::info!(
                "[integrations] Installed {}/{} skill at {:?}",
                agent,
                skill_name,
                target
            );
        } else {
            // Dedicated directory per skill (e.g. .claude/skills/vibearound/)
            let skill_dir = skill_base.join(skill_name);
            let target = skill_dir.join("SKILL.md");
            let _ = std::fs::create_dir_all(&skill_dir);
            std::fs::write(&target, content).with_context(|| format!("Write {:?}", target))?;
            tracing::info!(
                "[integrations] Installed {}/{} skill at {:?}",
                agent,
                skill_name,
                target
            );
        }
    }
    Ok(())
}

/// Remove all skill files for a given agent.
/// If `skill_filename` is set, removes only skill-specific files (shared
/// directories like `.cursor/rules/` may contain other user files).
/// Otherwise, removes each skill's dedicated directory.
pub(super) fn uninstall_skill(agent: &str) -> anyhow::Result<()> {
    let agent_def = match resources::agent_by_id(agent) {
        Some(def) => def,
        None => return Ok(()),
    };
    let global_config = match &agent_def.global_config {
        Some(cfg) => cfg,
        None => return Ok(()),
    };
    let skill_dir_rel = match &global_config.skill_dir {
        Some(dir) => dir,
        None => return Ok(()),
    };

    let home = home_dir()?;
    let primary_skill_dir = home.join(skill_dir_rel);
    let has_skill_filename = global_config.skill_filename.is_some();
    let skill_base = if has_skill_filename {
        primary_skill_dir.clone()
    } else {
        primary_skill_dir
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or(primary_skill_dir.clone())
    };

    for (skill_name, _) in agent_skills(agent) {
        if has_skill_filename {
            let ext = global_config
                .skill_filename
                .as_deref()
                .and_then(|f| f.rsplit('.').next())
                .unwrap_or("md");
            let filename = format!("{}.{}", skill_name, ext);
            let target = skill_base.join(&filename);
            if target.exists() {
                let _ = std::fs::remove_file(&target);
                tracing::info!(
                    "[integrations] Removed {}/{} skill at {:?}",
                    agent,
                    skill_name,
                    target
                );
            }
        } else {
            let skill_dir = skill_base.join(skill_name);
            if skill_dir.exists() {
                let _ = std::fs::remove_dir_all(&skill_dir);
                tracing::info!(
                    "[integrations] Removed {}/{} skill at {:?}",
                    agent,
                    skill_name,
                    skill_dir
                );
            }
        }
    }
    Ok(())
}

/// All skills to deploy, per agent. Returns (skill_name, content) pairs.
/// `skill_name` is used to derive both the target directory and filename.
///
/// Each agent gets the same set of skills; only the directory (and thus the
/// embedded content) differs. The macro eliminates 7× repetition of the
/// skill-name list.
fn agent_skills(agent: &str) -> Vec<(&'static str, &'static str)> {
    macro_rules! skills_for {
        ($dir:literal) => {
            vec![
                (
                    "vibearound",
                    include_str!(concat!("../../../skills/", $dir, "/vibearound/SKILL.md")),
                ),
                (
                    "va-session",
                    include_str!(concat!("../../../skills/", $dir, "/va-session/SKILL.md")),
                ),
                (
                    "va-preview",
                    include_str!(concat!("../../../skills/", $dir, "/va-preview/SKILL.md")),
                ),
                (
                    "va-md-preview",
                    include_str!(concat!("../../../skills/", $dir, "/va-md-preview/SKILL.md")),
                ),
            ]
        };
    }

    let mut skills = match agent {
        "claude" => skills_for!("claude"),
        "gemini" => skills_for!("gemini"),
        "codex" => skills_for!("codex"),
        "cursor" => skills_for!("cursor"),
        "kiro" => skills_for!("kiro"),
        "qwen-code" => skills_for!("qwen-code"),
        // Generic fallback — top-level skills dir (no agent subdirectory).
        _ => vec![
            (
                "vibearound",
                include_str!("../../../skills/vibearound/SKILL.md"),
            ),
            (
                "va-session",
                include_str!("../../../skills/va-session/SKILL.md"),
            ),
            (
                "va-preview",
                include_str!("../../../skills/va-preview/SKILL.md"),
            ),
            (
                "va-md-preview",
                include_str!("../../../skills/va-md-preview/SKILL.md"),
            ),
        ],
    };

    match agent {
        "claude" => skills.push((
            "agent-collaboration",
            include_str!("../../../skills/claude/agent-collaboration/SKILL.md"),
        )),
        "codex" => skills.push((
            "agent-collaboration",
            include_str!("../../../skills/codex/agent-collaboration/SKILL.md"),
        )),
        _ => {}
    }

    skills
}
