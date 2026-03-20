//! IM worker: receive InboundMessage, dispatch to agents via ServiceManager.
//!
//! Architecture:
//! - A Manager Agent is auto-spawned on first message (default CLI agent with manager prompt).
//! - All messages route to the current target agent (Manager by default).
//! - `/switch <worker_id>` routes subsequent messages to a specific Worker Agent.
//! - `/back` returns routing to the Manager Agent.
//! - `/workers` lists all agents. `/spawn` creates a new worker. `/kill` stops one.

use std::path::PathBuf;
use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::mpsc;

use super::daemon::{OutboundHub, OutboundMsg};
use crate::agent::{self, AgentEvent, AgentKind};
use crate::agent::registry::{self, AgentId};
use crate::config::{self, ImVerboseConfig};
use crate::service::{AgentRole, ServiceManager};

/// Attachment metadata from Feishu file/image messages.
#[derive(Debug, Clone)]
pub struct FeishuAttachment {
    pub message_id: String,
    pub file_key: String,
    pub file_name: String,
    /// "file" or "image"
    pub resource_type: String,
}

/// Inbound message from any IM channel to the worker.
#[derive(Debug, Clone)]
pub struct InboundMessage {
    pub channel_id: String,
    pub text: String,
    pub attachments: Vec<FeishuAttachment>,
    pub parent_id: Option<String>,
    /// The platform message_id of the user's message (for reactions and reply-quoting).
    pub user_message_id: Option<String>,
}

impl InboundMessage {
    pub fn text_only(channel_id: String, text: String) -> Self {
        Self { channel_id, text, attachments: vec![], parent_id: None, user_message_id: None }
    }
}

pub async fn run_worker<T>(
    mut inbound_rx: mpsc::Receiver<InboundMessage>,
    outbound: Arc<OutboundHub<T>>,
    busy_set: Arc<DashMap<String, ()>>,
    verbose: ImVerboseConfig,
    services: Arc<ServiceManager>,
) where
    T: crate::im::transport::ImTransport + 'static,
{
    // Start message hub — persists all agent events to JSONL session files
    let mut hub_tx: Option<mpsc::UnboundedSender<super::message_hub::HubMessage>> = {
        let sessions_dir = super::session_store::manager_sessions_dir();
        Some(super::message_hub::spawn_hub(sessions_dir))
    };

    // Current target agent id — None until Manager is spawned on first message.
    let mut current_target: Option<AgentId> = None;
    // Manager agent id — set once on first spawn.
    let mut manager_id: Option<AgentId> = None;

    while let Some(msg) = inbound_rx.recv().await {
        let channel_id = msg.channel_id.clone();
        busy_set.insert(channel_id.clone(), ());

        // --- Command routing ---
        let text = msg.text.trim();

        if text == "/help" {
            send_help(&channel_id, &outbound).await;
            busy_set.remove(&channel_id);
            continue;
        }

        if text == "/start" {
            send_start(&channel_id, &outbound).await;
            busy_set.remove(&channel_id);
            continue;
        }

        if text == "/workers" {
            send_workers_list(&channel_id, &outbound, &services).await;
            busy_set.remove(&channel_id);
            continue;
        }

        if text == "/back" {
            if let Some(ref mid) = manager_id {
                current_target = Some(mid.clone());
                let _ = outbound.send_direct(&channel_id, "Switched back to Manager Agent ✅").await;
            } else {
                let _ = outbound.send_direct(&channel_id, "No Manager Agent running yet.").await;
            }
            busy_set.remove(&channel_id);
            continue;
        }

        if let Some(rest) = text.strip_prefix("/switch ") {
            let worker_id = rest.trim();
            if services.agents.contains_key(worker_id) {
                current_target = Some(worker_id.to_string());
                let _ = outbound.send_direct(&channel_id, &format!("Switched to `{}` ✅", worker_id)).await;
            } else {
                let _ = outbound.send_direct(&channel_id, &format!("Worker `{}` not found. Use /workers to list.", worker_id)).await;
            }
            busy_set.remove(&channel_id);
            continue;
        }

        if let Some(rest) = text.strip_prefix("/spawn ") {
            handle_spawn_command(rest, &channel_id, &outbound, &services).await;
            busy_set.remove(&channel_id);
            continue;
        }

        if text == "/new" || text.starts_with("/new ") {
            // Kill current Manager agent if running
            if let Some(ref mid) = manager_id {
                let _ = services.agents.remove(mid);
                eprintln!("[worker] /new: killed Manager agent {}", mid);
            }
            manager_id = None;
            current_target = None;

            // Restart the message hub with a fresh sessions dir
            let sessions_dir = super::session_store::manager_sessions_dir();
            hub_tx = Some(super::message_hub::spawn_hub(sessions_dir));

            let summary = text.strip_prefix("/new ").map(|s| s.trim()).filter(|s| !s.is_empty());
            let msg = if let Some(s) = summary {
                format!("New session started: {} ✅", s)
            } else {
                "New session started ✅".to_string()
            };
            let _ = outbound.send_direct(&channel_id, &msg).await;
            busy_set.remove(&channel_id);
            continue;
        }

        if let Some(rest) = text.strip_prefix("/kill ") {
            handle_kill_command(rest.trim(), &channel_id, &outbound, &services).await;
            busy_set.remove(&channel_id);
            continue;
        }

        // --- Legacy /cli_<agent> command: spawn as worker in default workspace ---
        if let Some(kind) = parse_cli_command(text) {
            let workspace = config::ensure_loaded().working_dir.clone();
            match registry::spawn_agent(&services, kind, workspace, AgentRole::Worker).await {
                Ok(id) => {
                    current_target = Some(id.clone());
                    let _ = outbound.send_direct(&channel_id, &format!("{} worker started, switched to it ✅", kind)).await;
                }
                Err(e) => {
                    let _ = outbound.send_direct(&channel_id, &format!("Failed to start {} worker: {}", kind, e)).await;
                }
            }
            busy_set.remove(&channel_id);
            continue;
        }

        // --- Ensure Manager Agent is running (lazy start on first message) ---
        if current_target.is_none() {
            let default_kind = agent::AgentKind::from_str_loose(&config::ensure_loaded().default_agent)
                .unwrap_or(AgentKind::Claude);
            let workspace = config::data_dir(); // Manager works in ~/.vibearound/
            let status_mid = outbound.send_direct(&channel_id, &format!("Starting {} Manager Agent...", default_kind)).await;
            match registry::spawn_agent(&services, default_kind, workspace, AgentRole::Manager).await {
                Ok(id) => {
                    manager_id = Some(id.clone());
                    current_target = Some(id);
                    if let Some(ref mid) = status_mid {
                        outbound.edit_direct(&channel_id, mid, &format!("{} Manager Agent started ✅", default_kind)).await;
                    }
                }
                Err(e) => {
                    if let Some(ref mid) = status_mid {
                        outbound.edit_direct(&channel_id, mid, &format!("Failed to start Manager Agent: {}", e)).await;
                    }
                    busy_set.remove(&channel_id);
                    continue;
                }
            }
        }

        // --- Send message to current target agent ---
        let target_id = current_target.as_ref().unwrap();
        let agent_died = run_with_agent_from_registry(
            &services, target_id, &msg, &channel_id, &outbound, &verbose, &hub_tx,
        ).await;

        // --- Auto-restart if agent process died ---
        if agent_died {
            let _ = outbound.send_direct(&channel_id, "⚠️ Agent crashed, attempting restart...").await;
            // Try to get the kind and workspace from the entry before removing
            let restart_info = services.agents.get(target_id).map(|e| (e.kind, e.workspace.clone(), e.role));
            if let Some((kind, workspace, role)) = restart_info {
                // Remove stale entry
                services.agents.remove(target_id);
                match registry::spawn_agent(&services, kind, workspace, role).await {
                    Ok(new_id) => {
                        if role == AgentRole::Manager {
                            manager_id = Some(new_id.clone());
                        }
                        current_target = Some(new_id);
                        let _ = outbound.send_direct(&channel_id, &format!("{} agent restarted ✅", kind)).await;
                    }
                    Err(e) => {
                        let _ = outbound.send_direct(&channel_id, &format!("Failed to restart agent: {}", e)).await;
                        current_target = None;
                    }
                }
            }
        }

        busy_set.remove(&channel_id);
    }
}

/// Parse `/cli_<agent>` command. Returns the AgentKind if matched and enabled.
fn parse_cli_command(text: &str) -> Option<AgentKind> {
    let text = text.trim();
    let rest = text.strip_prefix("/cli_")?;
    let kind = AgentKind::from_str_loose(rest.trim())?;
    if kind.is_enabled() { Some(kind) } else { None }
}

/// Handle `/spawn <kind> <workspace>` command.
async fn handle_spawn_command<T>(
    args: &str,
    channel_id: &str,
    outbound: &Arc<OutboundHub<T>>,
    services: &Arc<ServiceManager>,
) where
    T: crate::im::transport::ImTransport + 'static,
{
    let parts: Vec<&str> = args.trim().splitn(2, ' ').collect();
    if parts.is_empty() {
        let _ = outbound.send_direct(channel_id, "Usage: `/spawn <kind> [workspace_path]`").await;
        return;
    }
    let kind = match AgentKind::from_str_loose(parts[0]) {
        Some(k) => k,
        None => {
            let _ = outbound.send_direct(channel_id, &format!("Unknown agent kind: `{}`", parts[0])).await;
            return;
        }
    };
    let workspace = if parts.len() > 1 {
        PathBuf::from(parts[1].trim())
    } else {
        // Default: create a new workspace under ~/.vibearound/workspaces/
        let name = format!("workspace-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S"));
        config::data_dir().join("workspaces").join(name)
    };

    let status_mid = outbound.send_direct(channel_id, &format!("Spawning {} worker at `{}`...", kind, workspace.display())).await;
    match registry::spawn_agent(services, kind, workspace, AgentRole::Worker).await {
        Ok(id) => {
            if let Some(ref mid) = status_mid {
                outbound.edit_direct(channel_id, mid, &format!("Worker `{}` started ✅", id)).await;
            }
        }
        Err(e) => {
            if let Some(ref mid) = status_mid {
                outbound.edit_direct(channel_id, mid, &format!("Failed to spawn worker: {}", e)).await;
            }
        }
    }
}

/// Handle `/kill <worker_id>` command.
async fn handle_kill_command<T>(
    worker_id: &str,
    channel_id: &str,
    outbound: &Arc<OutboundHub<T>>,
    services: &Arc<ServiceManager>,
) where
    T: crate::im::transport::ImTransport + 'static,
{
    match registry::kill_agent(services, worker_id).await {
        Ok(()) => {
            let _ = outbound.send_direct(channel_id, &format!("Worker `{}` killed ✅", worker_id)).await;
        }
        Err(e) => {
            let _ = outbound.send_direct(channel_id, &format!("Failed to kill `{}`: {}", worker_id, e)).await;
        }
    }
}

/// Send a /workers list showing all agents and their status.
async fn send_workers_list<T>(
    channel_id: &str,
    outbound: &Arc<OutboundHub<T>>,
    services: &Arc<ServiceManager>,
) where
    T: crate::im::transport::ImTransport + 'static,
{
    let agents = registry::list_agents(services);
    if agents.is_empty() {
        let _ = outbound.send_direct(channel_id, "No agents running.").await;
        return;
    }
    let mut lines = Vec::new();
    for a in &agents {
        let role_icon = match a.role {
            AgentRole::Manager => "👑",
            AgentRole::Worker => "🔧",
        };
        let status_icon = if a.status == "running" { "●" } else { "○" };
        lines.push(format!(
            "{} {} `{}` — {} {} ({}s)",
            role_icon, status_icon, a.id, a.kind, a.status, a.uptime_secs
        ));
    }
    let _ = outbound.send_direct(channel_id, &lines.join("\n")).await;
}

/// Send a message to an agent from the registry and stream events back to IM.
/// Subscribes to ALL agents (Manager + Workers) so worker activity is visible too.
/// Returns `true` if the agent process died (caller should restart).
async fn run_with_agent_from_registry<T>(
    services: &Arc<ServiceManager>,
    agent_id: &str,
    msg: &InboundMessage,
    channel_id: &str,
    outbound: &Arc<OutboundHub<T>>,
    verbose: &ImVerboseConfig,
    hub_tx: &Option<mpsc::UnboundedSender<super::message_hub::HubMessage>>,
) -> bool
where
    T: crate::im::transport::ImTransport + 'static,
{
    // Subscribe to the target agent + fire prompt
    let manager_rx = {
        let entry = match services.agents.get(agent_id) {
            Some(e) => e,
            None => {
                let _ = outbound.send_direct(channel_id, &format!("Agent `{}` not found", agent_id)).await;
                return false;
            }
        };
        let backend = match entry.backend.as_ref() {
            Some(b) => b,
            None => {
                let _ = outbound.send_direct(channel_id, &format!("Agent `{}` has no backend", agent_id)).await;
                return false;
            }
        };
        let rx = backend.subscribe();
        if let Err(e) = backend.send_message_fire(&msg.text).await {
            let is_shutdown = e.contains("shut down") || e.contains("gone") || e.contains("ACP thread");
            let _ = outbound.send(channel_id, OutboundMsg::StreamPart(
                channel_id.to_string(), format!("[Agent error: {}]", e),
            )).await;
            let _ = outbound.send(channel_id, OutboundMsg::StreamDone(channel_id.to_string())).await;
            return is_shutdown;
        }
        rx
    };

    // Send a "Working..." placeholder via stream progress so the user sees immediate feedback.
    // This integrates with the daemon's state machine — the first real StreamPart will replace it.
    let _ = outbound.send(channel_id, OutboundMsg::StreamProgress(
        channel_id.to_string(), "⏳ Working...".to_string(),
    )).await;

    // Stream all agent events (Manager + Workers) back to IM in real-time
    let agent_died = stream_all_agent_events(
        services, agent_id, manager_rx, channel_id, outbound, verbose,
        msg.user_message_id.as_deref(), hub_tx,
    ).await;

    agent_died
}

/// Stream events from ALL agents (Manager + Workers) back to IM.
/// Watches for new agents via ServiceManager change notifications.
/// Returns `true` if the primary agent died.
async fn stream_all_agent_events<T>(
    services: &Arc<ServiceManager>,
    primary_agent_id: &str,
    primary_rx: tokio::sync::broadcast::Receiver<AgentEvent>,
    channel_id: &str,
    outbound: &Arc<OutboundHub<T>>,
    verbose: &ImVerboseConfig,
    user_message_id: Option<&str>,
    hub_tx: &Option<mpsc::UnboundedSender<super::message_hub::HubMessage>>,
) -> bool
where
    T: crate::im::transport::ImTransport + 'static,
{
    use tokio::sync::broadcast::error::RecvError;

    let caps = outbound.capabilities();
    let mut agent_died = false;

    if let Some(user_mid) = user_message_id {
        outbound.set_reply_to(channel_id, user_mid.to_string()).await;
    }
    if let Some(user_mid) = user_message_id {
        let _ = outbound.send(channel_id, OutboundMsg::AddReaction(
            channel_id.to_string(), user_mid.to_string(), caps.processing_reaction.to_string(),
        )).await;
    }

    #[derive(PartialEq, Clone, Copy)]
    enum Block { None, Thinking, Text, Tool }
    let mut current_block = Block::None;

    async fn flush_block<T2: crate::im::transport::ImTransport + 'static>(
        channel_id: &str, outbound: &Arc<OutboundHub<T2>>,
    ) {
        let _ = outbound.send(channel_id, OutboundMsg::StreamEnd(channel_id.to_string())).await;
    }

    // Merge channel: all agent events funnel through here, tagged with agent_id
    let (merge_tx, mut merge_rx) = mpsc::unbounded_channel::<(String, AgentEvent)>();

    // Spawn reader for the primary (Manager) agent
    {
        let tx = merge_tx.clone();
        let aid = primary_agent_id.to_string();
        let mut rx = primary_rx;
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => { let _ = tx.send((aid.clone(), event)); }
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => break,
                }
            }
        });
    }

    // Track which agents we've already subscribed to
    let subscribed = Arc::new(tokio::sync::Mutex::new({
        let mut s = std::collections::HashSet::new();
        s.insert(primary_agent_id.to_string());
        s
    }));

    // Watch for new agents appearing in the registry
    let mut change_rx = services.change_tx.subscribe();
    let merge_tx_w = merge_tx.clone();
    let services_w = services.clone();
    let subscribed_w = subscribed.clone();
    tokio::spawn(async move {
        loop {
            match change_rx.recv().await {
                Ok(_) => {
                    let mut sub = subscribed_w.lock().await;
                    for entry in services_w.agents.iter() {
                        let aid = entry.key().clone();
                        if sub.contains(&aid) { continue; }
                        if let Some(backend) = entry.backend.as_ref() {
                            let rx = backend.subscribe();
                            sub.insert(aid.clone());
                            let tx = merge_tx_w.clone();
                            tokio::spawn(async move {
                                let mut rx = rx;
                                loop {
                                    match rx.recv().await {
                                        Ok(event) => { let _ = tx.send((aid.clone(), event)); }
                                        Err(RecvError::Lagged(_)) => continue,
                                        Err(RecvError::Closed) => break,
                                    }
                                }
                            });
                        }
                    }
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    });

    // Drop our copy so merge_rx eventually closes when all senders are gone
    drop(merge_tx);

    let mut last_agent_id = primary_agent_id.to_string();

    while let Some((agent_id, event)) = merge_rx.recv().await {
        // Log every event
        if let Some(ref log_tx) = hub_tx {
            let _ = log_tx.send(super::message_hub::HubMessage::Event {
                chat_id: channel_id.to_string(),
                agent_id: agent_id.clone(),
                event: event.clone(),
            });
        }

        // Show agent label when switching between agents
        if agent_id != last_agent_id {
            if current_block != Block::None {
                flush_block(channel_id, outbound).await;
                current_block = Block::None;
            }
            let label = format!("── {} ──\n", short_agent_label(&agent_id));
            let _ = outbound.send(channel_id, OutboundMsg::StreamPart(
                channel_id.to_string(), label,
            )).await;
            last_agent_id = agent_id.clone();
        }

        match event {
            AgentEvent::Text(text) => {
                if current_block != Block::Text {
                    if current_block != Block::None {
                        flush_block(channel_id, outbound).await;
                    }
                    current_block = Block::Text;
                }
                let _ = outbound.send(channel_id, OutboundMsg::StreamPart(
                    channel_id.to_string(), text,
                )).await;
            }
            AgentEvent::Thinking(text) => {
                if !verbose.show_thinking { continue; }
                if current_block != Block::Thinking {
                    if current_block != Block::None {
                        flush_block(channel_id, outbound).await;
                    }
                    current_block = Block::Thinking;
                    let _ = outbound.send(channel_id, OutboundMsg::StreamPart(
                        channel_id.to_string(), "🧠 Thinking...\n".to_string(),
                    )).await;
                }
                let _ = outbound.send(channel_id, OutboundMsg::StreamPart(
                    channel_id.to_string(), text,
                )).await;
            }
            AgentEvent::Progress(status) => {
                let _ = outbound.send(channel_id, OutboundMsg::StreamProgress(
                    channel_id.to_string(), status,
                )).await;
            }
            AgentEvent::ToolUse { name, id: _, input } => {
                // Special handling for dispatch_task: show a placeholder instead of raw tool call
                if name == "dispatch_task" {
                    if current_block != Block::None {
                        flush_block(channel_id, outbound).await;
                    }
                    current_block = Block::Tool;
                    let _ = outbound.send(channel_id, OutboundMsg::StreamPart(
                        channel_id.to_string(), "⏳ Dispatching task to worker...\n".to_string(),
                    )).await;
                    continue;
                }
                if !verbose.show_tool_use { continue; }
                if current_block != Block::None {
                    flush_block(channel_id, outbound).await;
                }
                current_block = Block::Tool;
                let mut tool_msg = format!("🔧 **{}**", name);
                if let Some(ref inp) = input {
                    if !inp.is_empty() {
                        tool_msg.push_str(&format!("\n```\n{}\n```", inp));
                    }
                }
                tool_msg.push('\n');
                let _ = outbound.send(channel_id, OutboundMsg::StreamPart(
                    channel_id.to_string(), tool_msg,
                )).await;
            }
            AgentEvent::ToolResult { id: _, output: _, is_error: _ } => {
                // Tool results are for the LLM, not the user — skip display
                continue;
            }
            AgentEvent::TurnComplete { .. } => {
                if agent_id == primary_agent_id {
                    break;
                }
                // Worker turn complete — flush block, continue waiting for Manager
                if current_block != Block::None {
                    flush_block(channel_id, outbound).await;
                    current_block = Block::None;
                }
            }
            AgentEvent::Error(err) => {
                let _ = outbound.send(channel_id, OutboundMsg::StreamPart(
                    channel_id.to_string(), format!("[Error: {}]\n", err),
                )).await;
                if agent_id == primary_agent_id {
                    agent_died = true;
                    break;
                }
            }
        }
    }

    let _ = outbound.send(channel_id, OutboundMsg::StreamDone(channel_id.to_string())).await;

    if let Some(user_mid) = user_message_id {
        let _ = outbound.send(channel_id, OutboundMsg::RemoveReaction(
            channel_id.to_string(), user_mid.to_string(), caps.processing_reaction.to_string(),
        )).await;
    }

    agent_died
}

/// Short human-readable label for an agent_id like "claude:/path/to/workspace"
fn short_agent_label(agent_id: &str) -> String {
    if let Some((kind, path)) = agent_id.split_once(':') {
        let dir = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());
        format!("🤖 {} ({})", kind, dir)
    } else {
        format!("🤖 {}", agent_id)
    }
}

/// Send a /help card with all commands.
async fn send_help<T>(
    channel_id: &str,
    outbound: &Arc<OutboundHub<T>>,
) where
    T: crate::im::transport::ImTransport + 'static,
{
    let mut lines = vec![
        "**Commands:**".to_string(),
        "`/workers` — List all agents and their status".to_string(),
        "`/spawn <kind> [workspace]` — Create a new worker agent".to_string(),
        "`/switch <worker_id>` — Talk directly to a worker".to_string(),
        "`/back` — Return to Manager Agent".to_string(),
        "`/kill <worker_id>` — Stop a worker agent".to_string(),
        "`/start` — Quick agent picker".to_string(),
        "`/help` — Show this help".to_string(),
        String::new(),
        "**Legacy:**".to_string(),
    ];
    for kind in AgentKind::enabled() {
        lines.push(format!("`/cli_{}` — Spawn {} worker in default workspace", kind, kind));
    }
    let prompt = lines.join("\n");

    let _ = outbound.send(channel_id, OutboundMsg::SendInteractive {
        channel_id: channel_id.to_string(),
        prompt,
        options: vec![],
        reply_to: None,
    }).await;
}

/// Send a /start interactive card — compact agent picker.
async fn send_start<T>(
    channel_id: &str,
    outbound: &Arc<OutboundHub<T>>,
) where
    T: crate::im::transport::ImTransport + 'static,
{
    use crate::im::transport::{ButtonStyle, InteractiveOption};

    let default_kind = AgentKind::from_str_loose(&config::ensure_loaded().default_agent)
        .unwrap_or(AgentKind::Claude);

    let prompt = "**Select an agent to start:**";
    let mut options: Vec<InteractiveOption> = AgentKind::enabled()
        .iter()
        .map(|kind| {
            let style = if *kind == default_kind { ButtonStyle::Primary } else { ButtonStyle::Default };
            InteractiveOption {
                label: format!("{}", kind).to_string(),
                value: format!("/cli_{}", kind),
                style,
                group: 0,
            }
        })
        .collect();
    for opt in &mut options {
        if let Some(first) = opt.label.get_mut(..1) {
            first.make_ascii_uppercase();
        }
    }
    options.push(InteractiveOption { label: "❓ Help".into(), value: "/help".into(), style: ButtonStyle::Default, group: 1 });

    let _ = outbound.send_interactive_direct(channel_id, prompt, &options, None).await;
}
