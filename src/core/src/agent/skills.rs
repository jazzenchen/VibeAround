//! Skill file install/uninstall.
//!
//! Each agent gets the common VibeAround skills (`vibearound`, `va-session`,
//! `va-preview`); selected agents can receive additional skills while their
//! workflows are being validated.
//!
//! The `include_str!` paths are relative to this source file: `src/core/
//! src/agent/skills.rs` → `../../../skills/...` reaches the top-level
//! `src/skills/` directory where the skill markdown lives.

use std::path::Path;

use anyhow::Context;

use crate::resources;

use super::mcp::home_dir;

const OLD_MANAGED_SKILL_NAMES: &[&str] = &["va-md-preview"];

/// Replace all VibeAround-reserved skill files for one project/workspace.
pub(super) fn sync_project_skill(agent: &str, workspace: &Path) -> anyhow::Result<()> {
    let agent_def = match resources::agent_by_id(agent) {
        Some(def) => def,
        None => return Ok(()),
    };
    let global_config = match &agent_def.global_config {
        Some(cfg) => cfg,
        None => return Ok(()),
    };
    let skill_dir_rel = match skill_dir_for_scope(global_config, true) {
        Some(dir) => dir,
        None => return Ok(()),
    };

    sync_skill_at_root(agent, global_config, workspace, skill_dir_rel)
}

fn sync_skill_at_root(
    agent: &str,
    global_config: &resources::AgentGlobalConfig,
    root: &Path,
    skill_dir_rel: &str,
) -> anyhow::Result<()> {
    let current_skill_base = skill_base(root, global_config, skill_dir_rel);
    let mut cleanup_bases = vec![current_skill_base.clone()];
    if let Some(old_dir) = global_config
        .skill_dir
        .as_deref()
        .filter(|old_dir| *old_dir != skill_dir_rel)
    {
        cleanup_bases.push(skill_base(root, global_config, old_dir));
    }
    for cleanup_base in cleanup_bases {
        for skill_name in OLD_MANAGED_SKILL_NAMES {
            remove_old_skill(agent, global_config, &cleanup_base, skill_name)?;
        }
    }

    let has_skill_filename = global_config.skill_filename.is_some();
    for (skill_name, content) in agent_skills(agent) {
        if has_skill_filename {
            // Shared directory (e.g. .cursor/rules/) — use skill-specific filename
            let ext = global_config
                .skill_filename
                .as_deref()
                .and_then(|f| f.rsplit('.').next())
                .unwrap_or("md");
            let filename = format!("{}.{}", skill_name, ext);
            let target = current_skill_base.join(&filename);
            write_reserved_skill_file(agent, skill_name, &target, content)?;
        } else {
            // Dedicated directory per skill (e.g. .claude/skills/vibearound/)
            let skill_dir = current_skill_base.join(skill_name);
            let target = skill_dir.join("SKILL.md");
            write_reserved_skill_file(agent, skill_name, &target, content)?;
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
    let skill_dir_rel = match skill_dir_for_scope(global_config, false) {
        Some(dir) => dir,
        None => return Ok(()),
    };

    let home = home_dir()?;
    uninstall_skill_at_root(agent, global_config, &home, skill_dir_rel)
}

fn uninstall_skill_at_root(
    agent: &str,
    global_config: &resources::AgentGlobalConfig,
    root: &Path,
    skill_dir_rel: &str,
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

    for skill_name in OLD_MANAGED_SKILL_NAMES {
        remove_old_skill(agent, global_config, &skill_base, skill_name)?;
    }
    Ok(())
}

fn remove_old_skill(
    agent: &str,
    global_config: &resources::AgentGlobalConfig,
    skill_base: &Path,
    skill_name: &str,
) -> anyhow::Result<()> {
    let (target, dedicated_dir) = if let Some(filename) = &global_config.skill_filename {
        let ext = filename.rsplit('.').next().unwrap_or("md");
        (skill_base.join(format!("{skill_name}.{ext}")), None)
    } else {
        let dir = skill_base.join(skill_name);
        (dir.join("SKILL.md"), Some(dir))
    };

    if !target.is_file() {
        return Ok(());
    }

    std::fs::remove_file(&target).with_context(|| format!("Remove {:?}", target))?;
    if let Some(dir) = dedicated_dir {
        let is_empty = std::fs::read_dir(&dir)
            .with_context(|| format!("Read {:?}", dir))?
            .next()
            .transpose()
            .with_context(|| format!("Read {:?}", dir))?
            .is_none();
        if is_empty {
            std::fs::remove_dir(&dir).with_context(|| format!("Remove {:?}", dir))?;
        }
    }
    tracing::info!(
        "[integrations] Removed old {}/{} skill at {:?}",
        agent,
        skill_name,
        target
    );
    Ok(())
}

fn skill_dir_for_scope(
    global_config: &resources::AgentGlobalConfig,
    project_scoped: bool,
) -> Option<&str> {
    if project_scoped {
        global_config
            .project_skill_dir
            .as_deref()
            .or(global_config.skill_dir.as_deref())
    } else {
        global_config.skill_dir.as_deref()
    }
}

fn write_reserved_skill_file(
    agent: &str,
    skill_name: &str,
    target: &Path,
    content: &str,
) -> anyhow::Result<()> {
    if target.exists() && !target.is_file() {
        anyhow::bail!("reserved skill path is not a file: {}", target.display());
    }

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("Create {:?}", parent))?;
    }
    std::fs::write(target, content).with_context(|| format!("Write {:?}", target))?;
    tracing::info!(
        "[integrations] Installed {}/{} skill at {:?}",
        agent,
        skill_name,
        target
    );
    Ok(())
}

fn skill_base(
    root: &Path,
    global_config: &resources::AgentGlobalConfig,
    skill_dir_rel: &str,
) -> std::path::PathBuf {
    let primary_skill_dir = root.join(skill_dir_rel);
    if global_config.skill_filename.is_some() {
        primary_skill_dir
    } else {
        primary_skill_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or(primary_skill_dir)
    }
}

fn is_managed_skill_file(path: &Path) -> anyhow::Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    if !path.is_file() {
        return Ok(false);
    }
    let content = std::fs::read_to_string(path).with_context(|| format!("Read {:?}", path))?;
    Ok(content.contains("VibeAround")
        || content.contains("vibearound")
        || content.contains("_vibearound:")
        || content.contains("metadata: vibearound"))
}

/// Bundled `(skill_name, content)` pairs for an agent.
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
            ]
        };
    }

    let mut skills = match agent {
        "claude" => skills_for!("claude"),
        "gemini" => skills_for!("gemini"),
        "cursor" => skills_for!("cursor"),
        "kiro" => skills_for!("kiro"),
        "qwen-code" => skills_for!("qwen-code"),
        // Generic fallback — top-level skills dir (no agent subdirectory).
        // Agents that share the workspace-level `.agents/skills/` directory
        // (codex, va-agent) all use this so the files never disagree.
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;

    fn unique_test_dir(name: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "vibearound-skills-{name}-{}-{nonce}",
            std::process::id()
        ))
    }

    fn frontmatter_field<'a>(content: &'a str, field: &str) -> Option<&'a str> {
        let mut lines = content.lines();
        if lines.next()? != "---" {
            return None;
        }
        let prefix = format!("{field}:");
        for line in lines {
            if line == "---" {
                return None;
            }
            if let Some(value) = line.strip_prefix(&prefix) {
                return Some(value.trim());
            }
        }
        None
    }

    #[test]
    fn skill_frontmatter_descriptions_quote_mapping_colons() {
        for agent in ["claude", "codex", "gemini", "qwen-code", "cursor", "kiro", "va-agent"] {
            for (skill_name, content) in agent_skills(agent) {
                let Some(description) = frontmatter_field(content, "description") else {
                    continue;
                };
                if description.contains(": ") {
                    assert!(
                        description.starts_with('"') || description.starts_with('\''),
                        "{agent}/{skill_name} description contains an unquoted YAML mapping colon"
                    );
                }
            }
        }
    }

    #[test]
    fn active_preview_skill_covers_both_sources_without_the_retired_tool() {
        for agent in ["claude", "codex", "gemini", "qwen-code", "cursor", "kiro", "va-agent"] {
            let skills = agent_skills(agent);
            assert!(skills.iter().all(|(name, _)| *name != "va-md-preview"));
            let preview = skills
                .iter()
                .find(|(name, _)| *name == "va-preview")
                .unwrap()
                .1;
            assert!(preview.contains("port:"), "{agent} preview port source");
            assert!(preview.contains("file:"), "{agent} preview file source");
            assert!(!preview.contains("Tool: md_preview"));
        }
    }

    #[test]
    fn project_skill_sync_uses_agent_specific_locations() {
        let dir = unique_test_dir("matrix");
        fs::create_dir_all(&dir).unwrap();

        for (agent, expected) in [
            ("claude", ".claude/skills/va-session/SKILL.md"),
            ("codex", ".agents/skills/va-session/SKILL.md"),
            ("gemini", ".gemini/skills/va-session/SKILL.md"),
            ("qwen-code", ".qwen/skills/va-session/SKILL.md"),
            ("cursor", ".cursor/rules/va-session.mdc"),
            ("kiro", ".kiro/steering/va-session.md"),
        ] {
            sync_project_skill(agent, &dir).unwrap();
            assert!(
                dir.join(expected).exists(),
                "{agent} should install {expected}"
            );
        }
        assert!(!dir.join(".codex/skills/va-session/SKILL.md").exists());
        assert!(!dir.join(".codex/config.toml").exists());
        let codex_session_skill =
            fs::read_to_string(dir.join(".agents/skills/va-session/SKILL.md")).unwrap();
        assert!(codex_session_skill.contains("Do not inspect MCP resources"));
        assert!(codex_session_skill.contains("e.g. claude, codex, gemini"));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn agents_sharing_the_agents_skills_dir_get_identical_files() {
        // codex and va-agent both sync into `.agents/skills/`; the content must
        // be the same so alternating launches never overwrite each other with
        // agent-specific text.
        let dir = unique_test_dir("shared-agents-dir");
        fs::create_dir_all(&dir).unwrap();

        sync_project_skill("va-agent", &dir).unwrap();
        let va_agent_files: Vec<String> = ["va-session", "vibearound", "va-preview"]
            .iter()
            .map(|name| {
                fs::read_to_string(dir.join(format!(".agents/skills/{name}/SKILL.md"))).unwrap()
            })
            .collect();
        sync_project_skill("codex", &dir).unwrap();
        let codex_files: Vec<String> = ["va-session", "vibearound", "va-preview"]
            .iter()
            .map(|name| {
                fs::read_to_string(dir.join(format!(".agents/skills/{name}/SKILL.md"))).unwrap()
            })
            .collect();
        assert_eq!(va_agent_files, codex_files);

        // The shared session skill points at the built-in tool when present.
        assert!(va_agent_files[0].contains("VibeAround Agent exposes `get_session_id`"));
        assert!(va_agent_files[0].contains("Tool: va_mcp_get_session_id"));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn project_skill_sync_overwrites_reserved_names() {
        let dir = unique_test_dir("overwrite");
        let target = dir.join(".agents/skills/va-session/SKILL.md");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, "user-owned skill").unwrap();

        sync_project_skill("codex", &dir).unwrap();

        assert_ne!(fs::read_to_string(&target).unwrap(), "user-owned skill");
        assert!(dir.join(".agents/skills/vibearound/SKILL.md").exists());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn project_skill_sync_removes_old_names_from_current_and_former_dirs() {
        let dir = unique_test_dir("old-names");
        let current_dir = dir.join(".agents/skills/va-md-preview");
        let current_target = current_dir.join("SKILL.md");
        let old_target = dir.join(".codex/skills/va-md-preview/SKILL.md");
        let sidecar = current_dir.join("notes.txt");
        fs::create_dir_all(&current_dir).unwrap();
        fs::create_dir_all(old_target.parent().unwrap()).unwrap();
        fs::write(&current_target, "old current-path skill").unwrap();
        fs::write(&old_target, "old former-path skill").unwrap();
        fs::write(&sidecar, "user notes").unwrap();

        sync_project_skill("codex", &dir).unwrap();

        assert!(!current_target.exists());
        assert!(!old_target.exists());
        assert_eq!(fs::read_to_string(&sidecar).unwrap(), "user notes");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn project_skill_sync_removes_old_reserved_filename() {
        let dir = unique_test_dir("old-shared-name");
        let target = dir.join(".cursor/rules/va-md-preview.mdc");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(
            &target,
            "# VibeAround Markdown Preview\n\nuser-owned preview rule\n",
        )
        .unwrap();

        sync_project_skill("cursor", &dir).unwrap();

        assert!(!target.exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
