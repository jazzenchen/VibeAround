//! Agent registry: manages agent lifecycle independently from IM channels.
//!
//! Agents are registered in `ServiceManager.agents` and can be created, queried,
//! and killed via the registry. IM workers hold a reference to the target agent
//! by id rather than owning the agent directly.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::agent::{self, AgentKind};
use crate::service::{AgentRole, ServiceManager};

/// Unique agent identifier: "kind:workspace_path".
pub type AgentId = String;

/// Build the canonical agent id from kind and workspace.
pub fn agent_id(kind: AgentKind, workspace: &Path) -> AgentId {
    format!("{}:{}", kind, workspace.display())
}

/// Spawn a new agent, register it in the ServiceManager, and start it.
/// Returns the agent id on success.
pub async fn spawn_agent(
    services: &Arc<ServiceManager>,
    kind: AgentKind,
    workspace: PathBuf,
    role: AgentRole,
) -> Result<AgentId, String> {
    let id = agent_id(kind, &workspace);

    // Check if already exists and running
    if let Some(entry) = services.agents.get(&id) {
        if entry.meta.current_status().is_running() {
            return Err(format!("Agent {} is already running", id));
        }
        // Remove stale entry
        drop(entry);
        services.agents.remove(&id);
    }

    // Ensure workspace directory exists
    if !workspace.exists() {
        std::fs::create_dir_all(&workspace)
            .map_err(|e| format!("Failed to create workspace {:?}: {}", workspace, e))?;
    }

    // Write MCP config for this agent kind (so it can discover VibeAround's MCP server)
    let port = services.port;
    crate::agent::manager_prompt::ensure_mcp_config(kind, &workspace, port);

    // Create and start the backend
    let mut backend = agent::create_backend(kind);
    backend.start(&workspace).await?;

    // Register in ServiceManager
    let key = services.register_agent(kind, workspace, role, None);
    // Attach the live backend
    if let Some(mut entry) = services.agents.get_mut(&key) {
        entry.backend = Some(backend);
    }

    eprintln!("[VibeAround][agent-registry] spawned {} (role={:?})", key, role);
    services.notify_change();
    Ok(key)
}

/// Send a message to an agent by id. Returns the collected output text.
/// Waits for TurnComplete before returning.
pub async fn send_message(
    services: &Arc<ServiceManager>,
    agent_id: &str,
    message: &str,
) -> Result<String, String> {
    // Get a subscribe handle first, then fire (non-blocking)
    let rx = {
        let entry = services
            .agents
            .get(agent_id)
            .ok_or_else(|| format!("Agent {} not found", agent_id))?;
        let backend = entry
            .backend
            .as_ref()
            .ok_or_else(|| format!("Agent {} has no backend", agent_id))?;
        let rx = backend.subscribe();
        backend.send_message_fire(message).await?;
        rx
    };

    // Collect output until TurnComplete
    collect_output(rx).await
}

/// Collect agent output from a broadcast receiver until TurnComplete.
async fn collect_output(
    mut rx: tokio::sync::broadcast::Receiver<agent::AgentEvent>,
) -> Result<String, String> {
    let mut output = String::new();
    loop {
        match rx.recv().await {
            Ok(event) => match event {
                agent::AgentEvent::Text(text) => output.push_str(&text),
                agent::AgentEvent::TurnComplete { .. } => break,
                agent::AgentEvent::Error(err) => {
                    return Err(format!("Agent error: {}", err));
                }
                _ => {} // Ignore thinking, progress, tool events for collected output
            },
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                return Err("Agent process ended unexpectedly".into());
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                eprintln!("[VibeAround][agent-registry] event stream lagged by {}", n);
            }
        }
    }
    Ok(output)
}

/// Kill an agent by id. Shuts down the backend and marks it stopped.
pub async fn kill_agent(services: &Arc<ServiceManager>, agent_id: &str) -> Result<(), String> {
    let mut backend = {
        let mut entry = services
            .agents
            .get_mut(agent_id)
            .ok_or_else(|| format!("Agent {} not found", agent_id))?;
        entry.meta.mark_stopped("killed");
        entry.backend.take()
    };
    if let Some(ref mut b) = backend {
        b.shutdown().await;
    }
    eprintln!("[VibeAround][agent-registry] killed {}", agent_id);
    services.notify_change();
    Ok(())
}

/// List all agents with their status.
pub fn list_agents(services: &Arc<ServiceManager>) -> Vec<AgentInfo> {
    services
        .agents
        .iter()
        .map(|entry| AgentInfo {
            id: entry.key().clone(),
            kind: entry.kind.to_string(),
            workspace: entry.workspace.display().to_string(),
            role: entry.role,
            status: crate::service::status_string(&entry.meta.current_status()),
            uptime_secs: entry.meta.uptime_secs(),
        })
        .collect()
}

/// Serializable agent info for API/IM responses.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentInfo {
    pub id: String,
    pub kind: String,
    pub workspace: String,
    pub role: AgentRole,
    pub status: String,
    pub uptime_secs: u64,
}

// ---------------------------------------------------------------------------
// Workspace-based lookup (used by MCP send_to_worker)
// ---------------------------------------------------------------------------

/// Find a running agent by workspace path, optionally filtered by kind.
/// Returns the first matching running agent id.
pub fn find_by_workspace(
    services: &Arc<ServiceManager>,
    workspace: &Path,
    kind: Option<AgentKind>,
) -> Option<AgentId> {
    services
        .agents
        .iter()
        .filter(|e| e.workspace == workspace && e.meta.current_status().is_running())
        .filter(|e| kind.map_or(true, |k| e.kind == k))
        .map(|e| e.key().clone())
        .next()
}

/// Result of a send_to_worker operation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SendToWorkerResult {
    pub agent_id: AgentId,
    pub output: String,
    /// True if a new agent was auto-spawned for this request.
    pub spawned: bool,
}

/// High-level: send a message to a worker on a workspace.
/// Finds an existing running agent or auto-spawns one.
/// Returns the agent's collected output.
pub async fn send_to_worker(
    services: &Arc<ServiceManager>,
    workspace: PathBuf,
    message: &str,
    kind: Option<AgentKind>,
) -> Result<SendToWorkerResult, String> {
    let mut spawned = false;

    // 1. Try to find an existing running agent on this workspace
    let target = find_by_workspace(services, &workspace, kind);

    // 2. If not found, auto-spawn
    let agent_id = match target {
        Some(id) => id,
        None => {
            let spawn_kind = kind.unwrap_or_else(|| {
                agent::AgentKind::from_str_loose(&crate::config::ensure_loaded().default_agent)
                    .unwrap_or(AgentKind::Claude)
            });
            eprintln!(
                "[VibeAround][send_to_worker] no agent on {:?}, auto-spawning {}",
                workspace, spawn_kind
            );
            let id = spawn_agent(services, spawn_kind, workspace, AgentRole::Worker).await?;
            spawned = true;
            id
        }
    };

    // 3. Send message and collect output
    let output = send_message(services, &agent_id, message).await?;

    Ok(SendToWorkerResult {
        agent_id,
        output,
        spawned,
    })
}
