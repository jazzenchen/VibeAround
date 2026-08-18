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

use std::path::Path;

use anyhow::anyhow;

use crate::{config, resources};

pub use install::{
    auto_install_agent_cmd, auto_install_agent_cmd_with_output, auto_install_npm_agent,
    auto_install_npm_agent_with_output, auto_install_npm_agent_with_progress,
    auto_install_npm_agent_with_progress_and_cancel,
    auto_install_npm_global_package_with_progress_and_cancel,
    auto_install_npm_package_in_dir_with_progress_and_cancel, install_acp_agents,
    is_program_available, npm_package_bin_name, npm_package_installed,
    npm_package_installed_in_dir, InstallOutput,
};
pub use runtime::{
    acp_mcp_servers, AcpMcpError, AcpMcpServer, Agent, AgentClientHandler, AgentReady,
    StartupSession, VIBEAROUND_ACP_MCP_SERVER,
};

use mcp::{install_project_mcp_config, uninstall_mcp_config};
use skills::{sync_project_skill, uninstall_skill};

fn can_manage_project_files(agent: &str, workspace: &Path) -> anyhow::Result<bool> {
    if !workspace.is_dir() {
        anyhow::bail!("workspace does not exist: {}", workspace.display());
    }
    if workspace == config::home_dir() {
        tracing::info!(
            "[agent] skipping project integrations for {} in home directory {:?}",
            agent,
            workspace
        );
        return Ok(false);
    }
    Ok(true)
}

/// Replace VibeAround-reserved project skills with the bundled versions.
pub fn sync_project_skills(agent: &str, workspace: &Path) -> anyhow::Result<()> {
    if !can_manage_project_files(agent, workspace)? {
        return Ok(());
    }
    sync_project_skill(agent, workspace)
}

/// Write the current daemon's MCP-only credential into project config.
pub fn install_project_mcp(agent: &str, workspace: &Path) -> anyhow::Result<()> {
    if !can_manage_project_files(agent, workspace)? {
        return Ok(());
    }
    let Some(auth) = crate::auth::read_mcp_token_file() else {
        tracing::info!("[agent] auth-mcp.json missing; skipping project MCP config");
        return Ok(());
    };
    let mcp_url = format!("http://127.0.0.1:{}/va/mcp?token={}", auth.port, auth.token);
    install_project_mcp_config(agent, workspace, &mcp_url)
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
