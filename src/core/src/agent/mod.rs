//! Agent — one ACP-speaking coding CLI instance.
//!
//! An "agent" here is a concrete coding CLI (Claude, Codex, Gemini, Cursor…)
//! wired up to talk to VibeAround over ACP. Each live [`Conversation`] owns
//! at most one [`Agent`] at a time; switching/killing the CLI spawns a new
//! one.
//!
//! This module covers three responsibilities for one coding CLI:
//!
//! - **Runtime** ([`runtime`]) — [`Agent`] spawns the CLI process, wraps
//!   its stdio as an ACP connection, and exposes the northbound `acp::Agent`
//!   surface. Southbound events (`session_notification`,
//!   `request_permission`) go through [`AgentClientHandler`].
//! - **Install** ([`install`]) — auto-install missing agent binaries (npm
//!   packages or native CLIs with an install command). Called eagerly at
//!   onboarding and lazily on `Agent::spawn` miss.
//! - **Integrations** ([`mcp`], [`skills`]) — install the VibeAround MCP
//!   server URL and SKILL files. New launches use project-scoped workspace
//!   config; global helpers remain for cleanup of older installs.
//!
//! [`ThreadRuntime`]: crate::workspace::threads::ThreadRuntime

mod bridge;
pub mod install;
pub mod launch;
mod mcp;
pub mod runtime;
mod skills;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::anyhow;

use crate::{config, resources};

pub use install::{
    auto_install_agent_cmd, auto_install_agent_cmd_with_output, auto_install_npm_agent,
    auto_install_npm_agent_with_output, auto_install_npm_agent_with_progress,
    auto_install_npm_agent_with_progress_and_cancel, install_acp_agents, is_program_available,
    npm_package_bin_name, npm_package_installed, InstallOutput,
};
pub use runtime::{Agent, AgentClientHandler, AgentReady, StartupSession};

use mcp::{install_project_mcp_config, uninstall_mcp_config, uninstall_project_mcp_config};
use skills::{install_project_skill, uninstall_project_skill, uninstall_skill};

// ---------------------------------------------------------------------------
// Integration sync (MCP config + SKILL files)
// ---------------------------------------------------------------------------

/// Legacy global integration sync hook.
///
/// Runtime installation is project-scoped now and happens at launch time.
#[allow(dead_code)]
pub fn sync_integrations(_settings: &serde_json::Value) {
    tracing::info!("[agent] global integration sync skipped; using project-scoped launch install");
}

#[derive(Debug, Clone, Copy)]
pub struct ProjectIntegrationOptions {
    pub mcp: bool,
    pub skills: bool,
}

/// Install project-scoped integrations for the current agent/workspace.
pub fn install_project_integrations(
    agent: &str,
    workspace: &Path,
    options: ProjectIntegrationOptions,
) -> anyhow::Result<()> {
    if !workspace.is_dir() {
        anyhow::bail!("workspace does not exist: {}", workspace.display());
    }
    if workspace == config::home_dir() {
        tracing::info!(
            "[agent] skipping project integrations for {} in home directory {:?}",
            agent,
            workspace
        );
        return Ok(());
    }

    let mcp_url = current_mcp_url();
    if options.mcp {
        install_project_mcp_config(agent, workspace, &mcp_url)?;
    }
    if options.skills {
        install_project_skill(agent, workspace)?;
    }
    Ok(())
}

/// Install project-scoped integrations according to settings.json auto-install policy.
pub fn auto_install_project_integrations(agent: &str, workspace: &Path) -> anyhow::Result<()> {
    let cfg = config::ensure_loaded();
    install_project_integrations(
        agent,
        workspace,
        ProjectIntegrationOptions {
            mcp: cfg.integrations.mcp_auto_install,
            skills: cfg.integrations.skill_auto_install,
        },
    )
}

/// Remove VibeAround-managed integrations from global legacy locations and
/// every known project workspace.
pub fn uninstall_managed_integrations(remove_mcp: bool, remove_skills: bool) -> anyhow::Result<()> {
    let mut errors = Vec::new();
    let workspaces = known_integration_workspaces();
    for agent in resources::agent_ids() {
        if remove_mcp {
            if let Err(error) = uninstall_mcp_config(agent) {
                errors.push(format!("{} global MCP: {:#}", agent, error));
            }
            for workspace in &workspaces {
                if let Err(error) = uninstall_project_mcp_config(agent, workspace) {
                    errors.push(format!(
                        "{} project MCP {}: {:#}",
                        agent,
                        workspace.display(),
                        error
                    ));
                }
            }
        }
        if remove_skills {
            if let Err(error) = uninstall_skill(agent) {
                errors.push(format!("{} global skill: {:#}", agent, error));
            }
            for workspace in &workspaces {
                if let Err(error) = uninstall_project_skill(agent, workspace) {
                    errors.push(format!(
                        "{} project skill {}: {:#}",
                        agent,
                        workspace.display(),
                        error
                    ));
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(errors.join("\n")))
    }
}

/// Remove VibeAround-managed integrations from legacy global locations only.
pub fn uninstall_legacy_integrations(remove_mcp: bool, remove_skills: bool) -> anyhow::Result<()> {
    let mut errors = Vec::new();
    for agent in resources::agent_ids() {
        if remove_mcp {
            if let Err(error) = uninstall_mcp_config(agent) {
                errors.push(format!("{} legacy MCP: {:#}", agent, error));
            }
        }
        if remove_skills {
            if let Err(error) = uninstall_skill(agent) {
                errors.push(format!("{} legacy skill: {:#}", agent, error));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(errors.join("\n")))
    }
}

fn current_mcp_url() -> String {
    let port = config::DEFAULT_PORT;
    match crate::auth::read_token_file() {
        Some(auth) => format!("http://127.0.0.1:{}/va/mcp?token={}", port, auth.token),
        None => {
            tracing::info!(
                "[agent] auth.json missing — writing MCP config without token; \
                 coding agents will get 401 until the daemon rewrites it"
            );
            format!("http://127.0.0.1:{}/va/mcp", port)
        }
    }
}

fn known_integration_workspaces() -> Vec<PathBuf> {
    let cfg = config::ensure_loaded();
    let mut paths: BTreeSet<PathBuf> = cfg.all_workspaces().into_iter().collect();
    let agent_prefs = crate::agent_state::read_prefs();
    for preference in agent_prefs.agents.values() {
        if let Some(workspace) = &preference.workspace {
            paths.insert(workspace.clone());
        }
    }
    paths.into_iter().filter(|path| path.is_dir()).collect()
}

/// Resolve which agents are enabled from settings JSON.
/// Falls back to all agents if `enabled_agents` is not set.
pub fn resolve_enabled_agents(settings: &serde_json::Value, all_agents: &[&str]) -> Vec<String> {
    settings
        .get("enabled_agents")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_else(|| all_agents.iter().map(|s| s.to_string()).collect())
}
