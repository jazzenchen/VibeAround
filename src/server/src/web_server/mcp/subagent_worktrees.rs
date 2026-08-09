use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context};

use super::subagents::InitializeSubagentsArgs;

pub(super) struct InitializedSubagents {
    pub(super) turn: common::workspace::threads::MultiAgentTurn,
    pub(super) agents: Vec<common::workspace::threads::ThreadAgent>,
}

pub(super) fn initialize_subagent_worktrees(
    cwd: &Path,
    args: &InitializeSubagentsArgs,
    mode: common::workspace::threads::MultiAgentTurnMode,
) -> anyhow::Result<InitializedSubagents> {
    ensure_git_available()?;
    let repo_root = ensure_git_repository(cwd)?;
    let repo_root = common::workspace::normalize_workspace_cwd(repo_root);
    let head = ensure_git_head(&repo_root)?;
    let dirty = git_output(
        &repo_root,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?;
    if !dirty.trim().is_empty() {
        return Err(anyhow!(
            "Workspace has uncommitted or untracked changes. Commit, stash, or clean the workspace before initializing subagents."
        ));
    }
    let branch_prefix = clean_branch_prefix(args.branch_prefix.as_deref())?;
    let repo_slug = repo_root
        .file_name()
        .and_then(|name| name.to_str())
        .map(slugify)
        .filter(|slug| !slug.is_empty())
        .unwrap_or_else(|| "workspace".to_string());

    let turn_id = common::workspace::threads::MultiAgentTurnId::new();
    let short_turn = short_id(turn_id.as_str());
    let worktree_base = common::config::data_dir()
        .join("worktrees")
        .join(repo_slug)
        .join(turn_id.as_str());
    std::fs::create_dir_all(&worktree_base)
        .with_context(|| format!("create worktree base {}", worktree_base.display()))?;

    let mut agent_ids = Vec::with_capacity(args.agents.len());
    let mut agents = Vec::with_capacity(args.agents.len());

    for spec in &args.agents {
        let name = validate_agent_name(&spec.name)?;
        let agent_id = common::resources::resolve_agent_id(&spec.agent_kind)
            .map_err(|error| anyhow!(error))?;
        let subagent_id = common::workspace::threads::ThreadAgentId::new();
        let agent_short_id = short_id(subagent_id.as_str());
        let name_slug = slugify(&name);
        let branch = format!(
            "{}/{}/{}-{}",
            branch_prefix, short_turn, name_slug, agent_short_id
        );
        let worktree = worktree_base.join(format!("{}-{}", name_slug, agent_short_id));

        if let Err(error) = git_worktree_add(&repo_root, &branch, &worktree, &head) {
            cleanup_created_worktrees(&repo_root, &agents);
            return Err(error);
        }

        agent_ids.push(subagent_id.clone());
        agents.push(common::workspace::threads::ThreadAgent::ready(
            subagent_id,
            turn_id.clone(),
            name,
            agent_id,
            spec.profile_id.clone(),
            branch,
            worktree.to_string_lossy().to_string(),
            spec.task.clone().filter(|task| !task.trim().is_empty()),
        ));
    }

    Ok(InitializedSubagents {
        turn: common::workspace::threads::MultiAgentTurn::new(turn_id, mode, agent_ids),
        agents,
    })
}

fn ensure_git_available() -> anyhow::Result<()> {
    if command_success("git", &["--version"]) {
        return Ok(());
    }
    if try_install_git()? && command_success("git", &["--version"]) {
        return Ok(());
    }
    Err(anyhow!(
        "Git is required to initialize subagents, but `git` was not found on PATH."
    ))
}

fn try_install_git() -> anyhow::Result<bool> {
    if cfg!(target_os = "macos") && command_success("brew", &["--version"]) {
        let output = common::process::env::std_command("brew")
            .args(["install", "git"])
            .output()
            .context("install git with Homebrew")?;
        return Ok(output.status.success());
    }
    Ok(false)
}

fn command_success(program: &str, args: &[&str]) -> bool {
    common::process::env::std_command(program)
        .args(args)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn ensure_git_repository(cwd: &Path) -> anyhow::Result<PathBuf> {
    if let Ok(root) = git_output(cwd, &["rev-parse", "--show-toplevel"]) {
        return Ok(PathBuf::from(root));
    }
    let output = common::process::env::std_command("git")
        .arg("-C")
        .arg(cwd)
        .arg("init")
        .output()
        .with_context(|| format!("git init {}", cwd.display()))?;
    if !output.status.success() {
        return Err(anyhow!(
            "git init failed in {}: {}",
            cwd.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(PathBuf::from(git_output(
        cwd,
        &["rev-parse", "--show-toplevel"],
    )?))
}

fn ensure_git_head(repo_root: &Path) -> anyhow::Result<String> {
    if let Ok(head) = git_output(repo_root, &["rev-parse", "--verify", "HEAD"]) {
        return Ok(head);
    }
    let output = common::process::env::std_command("git")
        .arg("-C")
        .arg(repo_root)
        .args([
            "-c",
            "user.name=VibeAround",
            "-c",
            "user.email=vibearound@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "Initialize workspace for VibeAround subagents",
        ])
        .output()
        .with_context(|| format!("create initial git commit in {}", repo_root.display()))?;
    if !output.status.success() {
        return Err(anyhow!(
            "git initial commit failed in {}: {}",
            repo_root.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    git_output(repo_root, &["rev-parse", "--verify", "HEAD"])
}

fn validate_agent_name(name: &str) -> anyhow::Result<String> {
    let trimmed = name.trim();
    let char_count = trimmed.chars().count();
    if !(2..=64).contains(&char_count) {
        return Err(anyhow!("Subagent name must be 2-64 characters."));
    }
    if trimmed
        .chars()
        .any(|ch| ch.is_control() || matches!(ch, '/' | '\\'))
    {
        return Err(anyhow!(
            "Subagent name `{}` contains unsupported characters.",
            trimmed
        ));
    }
    if !trimmed.chars().any(|ch| ch.is_alphanumeric()) {
        return Err(anyhow!(
            "Subagent name `{}` must contain at least one letter or number.",
            trimmed
        ));
    }
    Ok(trimmed.to_string())
}

fn clean_branch_prefix(prefix: Option<&str>) -> anyhow::Result<String> {
    let prefix = prefix.unwrap_or("va/subagents").trim().trim_matches('/');
    if prefix.is_empty()
        || prefix.contains("..")
        || prefix
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace() || matches!(ch, '\\' | ':'))
    {
        return Err(anyhow!("Invalid branch_prefix `{}`.", prefix));
    }
    Ok(prefix.to_string())
}

fn git_output(cwd: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = common::process::env::std_command("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .with_context(|| format!("run git {}", args.join(" ")))?;
    if !output.status.success() {
        return Err(anyhow!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_worktree_add(cwd: &Path, branch: &str, worktree: &Path, head: &str) -> anyhow::Result<()> {
    let output = common::process::env::std_command("git")
        .arg("-C")
        .arg(cwd)
        .args(["worktree", "add", "-b", branch])
        .arg(worktree)
        .arg(head)
        .output()
        .with_context(|| format!("create git worktree {}", worktree.display()))?;
    if output.status.success() {
        return Ok(());
    }
    Err(anyhow!(
        "git worktree add failed for {}: {}",
        worktree.display(),
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

pub(super) fn cleanup_created_worktrees(
    repo: &Path,
    agents: &[common::workspace::threads::ThreadAgent],
) {
    for agent in agents.iter().rev() {
        let _ = common::process::env::std_command("git")
            .arg("-C")
            .arg(repo)
            .args(["worktree", "remove", "--force", &agent.worktree])
            .output();
        let _ = common::process::env::std_command("git")
            .arg("-C")
            .arg(repo)
            .args(["branch", "-D", &agent.branch])
            .output();
    }
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "agent".to_string()
    } else {
        slug
    }
}

fn short_id(id: &str) -> String {
    id.chars()
        .filter(|ch| ch.is_ascii_hexdigit())
        .take(8)
        .collect::<String>()
}
