//! Agent integration installation.

/// Project MCP config + skills are installed lazily for the active workspace
/// when an agent launches. Kept as a no-op compatibility wrapper for old
/// onboarding call sites.
#[allow(dead_code)]
pub(super) fn install_agent_integrations(_settings: &serde_json::Value) -> anyhow::Result<()> {
    Ok(())
}

/// Pre-install npm-based ACP agent packages for enabled agents.
#[allow(dead_code)]
pub(super) async fn install_acp_agents(settings: &serde_json::Value) {
    common::agent::install_acp_agents(settings).await;
}
