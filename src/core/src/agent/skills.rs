//! Skill file install/uninstall.
//!
//! Each agent gets the common VibeAround skills (`vibearound`, `va-session`,
//! `va-preview`, `va-md-preview`); selected agents can receive additional
//! skills while their workflows are being validated.
//!
//! The `include_str!` paths are relative to this source file: `src/core/
//! src/agent/skills.rs` → `../../../skills/...` reaches the top-level
//! `src/skills/` directory where the skill markdown lives.

use std::path::Path;

use anyhow::{anyhow, Context};

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
    install_skill_at_root(agent, global_config, &home, skill_dir_rel, false)
}

/// Install all skill files for a given agent into a project/workspace.
pub(super) fn install_project_skill(agent: &str, workspace: &Path) -> anyhow::Result<()> {
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

    install_skill_at_root(agent, global_config, workspace, skill_dir_rel, true)
}

fn install_skill_at_root(
    agent: &str,
    global_config: &resources::AgentGlobalConfig,
    root: &Path,
    skill_dir_rel: &str,
    project_scoped: bool,
) -> anyhow::Result<()> {
    let primary_skill_dir = root.join(skill_dir_rel);

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
            std::fs::create_dir_all(&skill_base)
                .with_context(|| format!("Create {:?}", skill_base))?;
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
            std::fs::create_dir_all(&skill_dir)
                .with_context(|| format!("Create {:?}", skill_dir))?;
            std::fs::write(&target, content).with_context(|| format!("Write {:?}", target))?;
            tracing::info!(
                "[integrations] Installed {}/{} skill at {:?}",
                agent,
                skill_name,
                target
            );
        }
    }
    if project_scoped && agent == "codex" {
        sync_codex_project_skill_config(root, &skill_base, true)?;
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
    uninstall_skill_at_root(agent, global_config, &home, skill_dir_rel, false)
}

/// Remove all project/workspace skill files for a given agent.
pub(super) fn uninstall_project_skill(agent: &str, workspace: &Path) -> anyhow::Result<()> {
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

    uninstall_skill_at_root(agent, global_config, workspace, skill_dir_rel, true)
}

fn uninstall_skill_at_root(
    agent: &str,
    global_config: &resources::AgentGlobalConfig,
    root: &Path,
    skill_dir_rel: &str,
    project_scoped: bool,
) -> anyhow::Result<()> {
    let primary_skill_dir = root.join(skill_dir_rel);
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
            if is_managed_skill_file(&target)? {
                std::fs::remove_file(&target).with_context(|| format!("Remove {:?}", target))?;
                tracing::info!(
                    "[integrations] Removed {}/{} skill at {:?}",
                    agent,
                    skill_name,
                    target
                );
            }
        } else {
            let skill_dir = skill_base.join(skill_name);
            let target = skill_dir.join("SKILL.md");
            if is_managed_skill_file(&target)? {
                std::fs::remove_dir_all(&skill_dir)
                    .with_context(|| format!("Remove {:?}", skill_dir))?;
                tracing::info!(
                    "[integrations] Removed {}/{} skill at {:?}",
                    agent,
                    skill_name,
                    skill_dir
                );
            }
        }
    }
    if project_scoped && agent == "codex" {
        sync_codex_project_skill_config(root, &skill_base, false)?;
    }
    Ok(())
}

fn is_managed_skill_file(path: &Path) -> anyhow::Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let content = std::fs::read_to_string(path).with_context(|| format!("Read {:?}", path))?;
    Ok(content.contains("VibeAround")
        || content.contains("vibearound")
        || content.contains("_vibearound:")
        || content.contains("metadata: vibearound"))
}

fn sync_codex_project_skill_config(
    workspace: &Path,
    skill_base: &Path,
    install: bool,
) -> anyhow::Result<()> {
    use toml_edit::{value, ArrayOfTables, DocumentMut, Item, Table};

    let config_path = workspace.join(".codex/config.toml");
    if !config_path.exists() && !install {
        return Ok(());
    }
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("Create {:?}", parent))?;
    }

    let data = match std::fs::read_to_string(&config_path) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error).with_context(|| format!("Read {:?}", config_path)),
    };
    let mut doc: DocumentMut = data
        .parse::<DocumentMut>()
        .with_context(|| format!("Parse TOML {:?}", config_path))?;

    if !doc.contains_key("skills") {
        doc["skills"] = Item::Table(Table::new());
    }
    let skills = doc["skills"]
        .as_table_mut()
        .ok_or_else(|| anyhow!("skills is not a table in {:?}", config_path))?;
    if !skills.contains_key("config") {
        skills["config"] = Item::ArrayOfTables(ArrayOfTables::new());
    }
    let config_item = skills
        .get_mut("config")
        .ok_or_else(|| anyhow!("skills.config missing in {:?}", config_path))?;
    let config = config_item.as_array_of_tables_mut().ok_or_else(|| {
        anyhow!(
            "skills.config is not an array of tables in {:?}",
            config_path
        )
    })?;

    let expected_paths: Vec<String> = agent_skills("codex")
        .into_iter()
        .map(|(skill_name, _)| skill_base.join(skill_name).to_string_lossy().to_string())
        .collect();

    let mut next = ArrayOfTables::new();
    for table in config.iter() {
        let path = table.get("path").and_then(|item| item.as_str());
        if path.map(|path| expected_paths.iter().any(|p| p == path)) != Some(true) {
            next.push(table.clone());
        }
    }

    if install {
        for path in expected_paths {
            let mut table = Table::new();
            table["path"] = value(path);
            table["enabled"] = value(true);
            next.push(table);
        }
    }

    *config_item = Item::ArrayOfTables(next);
    std::fs::write(&config_path, doc.to_string())
        .with_context(|| format!("Write {:?}", config_path))?;
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
