//! HTTP/WebSocket API response shapes for the dashboard.
//!
//! This module owns the **wire contract** between the server and its
//! frontends (web dashboard, Tauri desktop-ui, plus any future TUI / CLI
//! / third-party consumer). Types here exist only to be serialized.
//!
//! # Where the data comes from
//!
//! Structs in this module are populated by reading `common` core state
//! (via `config::ensure_loaded()` and `resources::...`). The core does
//! not know about HTTP; it exposes domain data and this module maps it
//! to wire shapes. Consumers that aren't HTTP (TUI, CLI) should write
//! their own mapping alongside core, not reuse these types.
//!
//! # Consumers
//!
//! The canonical TS validator/types live in
//! `src/shared/client-ts/src/schemas.ts` (zod). Keep the wire shapes
//! documented on each struct below so Python/Swift/curl consumers can
//! derive their own schemas without reading the zod file.

use serde::Serialize;

use common::previews::PreviewSnapshot;
use common::pty::{PtyRunState, PtyTool};

/// Per-agent display info returned under `AgentsConfig.agents`.
///
/// # Wire format (JSON)
/// ```json
/// { "id": "claude", "name": "Claude Code", "description": "Claude Code CLI" }
/// ```
///
/// - `id`: an agent ID from `resources/agents.json` (e.g. `"claude"`,
///   `"codex"`, `"pi"`, `"gemini"`, `"qwen-code"`).
/// - `name` / `description`: copied from that file's `display_name` and
///   `description` fields.
#[derive(Debug, Clone, Serialize)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub description: String,
}

/// `GET /api/agents` response envelope.
///
/// # Wire format (JSON)
/// ```json
/// {
///   "agents": [
///     { "id": "claude", "name": "Claude Code", "description": "..." },
///     { "id": "gemini", "name": "Gemini CLI",  "description": "..." }
///   ],
///   "default_agent": "claude"
/// }
/// ```
///
/// - `agents`: the enabled subset from settings.json (not all agents in
///   `agents.json`), ordered as configured.
/// - `default_agent`: raw string from settings.json. The server does not
///   cross-validate against `agents` — consumers should treat an
///   unrecognized value as "no default".
#[derive(Debug, Clone, Serialize)]
pub struct AgentsConfig {
    pub agents: Vec<AgentInfo>,
    pub default_agent: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProfileLaunchTarget {
    pub id: String,
    pub label: String,
    pub api_type: String,
    pub bridge_target_api_type: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProfileLaunchOption {
    pub id: String,
    pub label: String,
    pub provider: String,
    pub launch_targets: Vec<ProfileLaunchTarget>,
}

impl AgentInfo {
    /// Build an `AgentInfo` for each of the given agent IDs by looking up
    /// the corresponding entry in `agents.json`. IDs with no matching
    /// entry are silently dropped.
    pub fn for_ids(ids: &[String]) -> Vec<Self> {
        ids.iter()
            .filter_map(|id| {
                let def = common::resources::agent_by_id(id)?;
                Some(Self {
                    id: id.clone(),
                    name: def.display_name.clone(),
                    description: def.description.clone(),
                })
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Per-domain runtime shapes. Each is returned by a dedicated
// `/api/<domain>` handler reading directly from the relevant kernel
// manager — no unified snapshot envelope, no aggregate facade.
// ---------------------------------------------------------------------------

/// One channel plugin, as returned by `GET /api/channels`.
///
/// Sources: `common::channels::monitor::ChannelMonitor::list()`
///
/// # Wire format (JSON)
/// ```json
/// {
///   "kind": "telegram",
///   "status": "running",
///   "reason": null,
///   "crash_count": 0,
///   "last_seen_age_secs": 3,
///   "restart_in_secs": 0,
///   "started_at": 1713460000
/// }
/// ```
///
/// `status` is one of: `"not_started" | "spawning" | "running" | "crashed" | "stopped"`.
/// `reason` carries a short explanation for crashed/stopped states.
#[derive(Debug, Clone, Serialize)]
pub struct ChannelRuntime {
    pub kind: String,
    pub status: &'static str,
    pub reason: Option<String>,
    pub crash_count: u32,
    pub last_seen_age_secs: u64,
    pub restart_in_secs: u64,
    pub started_at: u64,
}

/// One tunnel, as returned by `GET /api/tunnels`.
///
/// Sources: `common::tunnels::TunnelManager::list()`.
///
/// # Wire format (JSON)
/// ```json
/// {
///   "provider": "localtunnel",
///   "url": "https://quiet-pig-42.loca.lt",
///   "status": { "state": "running" },
///   "uptime_secs": 120
/// }
/// ```
#[derive(Debug, Clone, Serialize)]
pub struct TunnelRuntime {
    pub provider: &'static str,
    pub url: Option<String>,
    pub status: common::tunnels::TunnelStatus,
    pub uptime_secs: u64,
}

/// One PTY session, as returned by `GET /api/sessions`.
#[derive(Debug, Clone, Serialize)]
pub struct SessionListItem {
    pub session_id: String,
    pub tool: PtyTool,
    pub status: PtyRunState,
    pub created_at: u64,
    pub project_path: Option<String>,
    pub profile_id: Option<String>,
    pub profile_label: Option<String>,
    pub launch_target: Option<String>,
    pub tmux_session: Option<String>,
}

/// `POST /api/sessions` response.
#[derive(Debug, Clone, Serialize)]
pub struct CreateSessionResponse {
    pub session_id: String,
    pub tool: PtyTool,
    pub created_at: u64,
    pub project_path: Option<String>,
    pub profile_id: Option<String>,
    pub profile_label: Option<String>,
    pub launch_target: Option<String>,
}

/// One resumable coding-agent session discovered from a CLI-owned session store.
#[derive(Debug, Clone, Serialize)]
pub struct LaunchSessionInfo {
    pub agent_id: String,
    pub session_id: String,
    pub title: String,
    pub workspace: String,
    pub updated_at: u64,
    pub short_id: String,
    pub archived: bool,
    pub active: bool,
}

/// `GET /api/tmux/sessions` response.
#[derive(Debug, Clone, Serialize)]
pub struct TmuxSessionsResponse {
    pub available: bool,
    pub sessions: Vec<String>,
}

/// One workspace entry, as returned by `GET /api/workspaces`.
#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceItem {
    pub path: String,
    pub is_default: bool,
    pub is_builtin: bool,
}

/// `GET /api/workspaces` response.
///
/// `default_workspace` is retained for wire compatibility; it always points
/// to the built-in workspace root.
#[derive(Debug, Clone, Serialize)]
pub struct WorkspacesResponse {
    pub workspaces: Vec<WorkspaceItem>,
    pub default_workspace: String,
}

/// One file uploaded from the web chat composer and staged for the next prompt.
#[derive(Debug, Clone, Serialize)]
pub struct ChatUploadResponse {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub size: u64,
    pub uri: String,
}

/// `POST /api/workspaces/create` response.
#[derive(Debug, Clone, Serialize)]
pub struct CreateWorkspaceResponse {
    pub workspace: WorkspaceItem,
    pub workspaces: Vec<WorkspaceItem>,
    pub default_workspace: String,
}

/// `GET /api/previews` response.
#[derive(Debug, Clone, Serialize)]
pub struct PreviewsResponse {
    pub previews: Vec<PreviewSnapshot>,
    pub tunnel_url: Option<String>,
}

// ---------------------------------------------------------------------------
// /ws/chat wire events
// ---------------------------------------------------------------------------

/// Every frame the `/ws/chat` handler pushes to the web dashboard. Tagged
/// by `kind` so the frontend does an exhaustive `switch` instead of
/// string-sniffing a free-form JSON blob.
///
/// Lifecycle events (config / agent_ready / session_ready /
/// command_menu / permission_request / system_text / error) are
/// dashboard meta — our own addition on top of ACP. Streaming tokens /
/// tool calls / turn completion all arrive as raw ACP
/// `SessionNotification` payloads under the `acp_notification` kind.
/// The frontend imports the matching TS types from
/// `@agentclientprotocol/sdk`, so there is no hand-written schema on
/// top of ACP.
///
/// # Wire format (JSON — examples)
/// ```json
/// { "kind": "config", "channel_id": "web:abc", "agents": [...], "default_agent": "claude" }
/// { "kind": "agent_ready", "agent": "Claude Code", "version": "1.0" }
/// { "kind": "session_ready", "session_id": "01HX..." }
/// { "kind": "session_mode", "session_mode": { "source": "config_option" } }
/// { "kind": "system_text", "text": "Session paired." }
/// { "kind": "acp_notification", "payload": { /* acp::SessionNotification */ } }
/// { "kind": "permission_request", "request_id": "pr-1", "request": { ... } }
/// { "kind": "multi_agent_turn", "turn": { ... }, "agents": [...] }
/// { "kind": "subagent_status", "agent": { ... } }
/// { "kind": "subagent_acp_notification", "agent": { ... }, "payload": { ... } }
/// { "kind": "command_menu", "system_commands": [...], "agent_commands": [...] }
/// { "kind": "prompt_done", "message_id": "01HX..." }
/// { "kind": "error", "error": "spawn failed: ..." }
/// ```
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatEvent {
    Config {
        channel_id: String,
        agents: Vec<AgentInfo>,
        default_agent: String,
    },
    AgentReady {
        agent: String,
        version: String,
    },
    SessionReady {
        session_id: String,
    },
    SessionMode {
        session_mode: serde_json::Value,
    },
    CommandMenu {
        system_commands: serde_json::Value,
        agent_commands: serde_json::Value,
    },
    PermissionRequest {
        request_id: String,
        request: serde_json::Value,
    },
    MultiAgentTurn {
        turn: common::workspace::threads::MultiAgentTurn,
        agents: Vec<common::workspace::threads::ThreadAgent>,
    },
    SubagentStatus {
        agent: common::workspace::threads::ThreadAgent,
    },
    SubagentAcpNotification {
        agent: common::workspace::threads::ThreadAgent,
        payload: serde_json::Value,
    },
    PromptDone {
        #[serde(skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
    },
    TurnStatus {
        active: bool,
    },
    SystemText {
        text: String,
    },
    /// Raw ACP payload. Consumers decode via
    /// `@agentclientprotocol/sdk`'s `SessionNotification` on the TS
    /// side, `acp::SessionNotification` on the Rust side.
    AcpNotification {
        payload: serde_json::Value,
    },
    Error {
        error: String,
    },
}

/// One agent runtime, as returned by `GET /api/agents/runtime`.
///
/// Sources: `WorkspaceThreadManager::list()` → live `ThreadRuntimeState`
/// entries. Persisted workspace threads that do not currently own a host
/// process are intentionally omitted.
///
/// # Wire format (JSON)
/// ```json
/// {
///   "route_key": "telegram:chat_42",
///   "channel_kind": "telegram",
///   "chat_id": "chat_42",
///   "cli_kind": "claude",
///   "profile": "default",
///   "session_id": "01HXYZ...",
///   "workspace": "/Users/foo/bar",
///   "busy": false,
///   "failed": null,
///   "started_at": 1713460000,
///   "agent_name": "Claude Code",
///   "agent_title": "Claude",
///   "agent_version": "1.0.0"
/// }
/// ```
#[derive(Debug, Clone, Serialize)]
pub struct AgentRuntime {
    pub route_key: String,
    pub channel_kind: String,
    pub chat_id: String,
    pub cli_kind: Option<String>,
    pub profile: Option<String>,
    pub session_id: Option<String>,
    pub workspace: Option<String>,
    pub busy: bool,
    pub failed: Option<String>,
    pub started_at: u64,
    pub agent_name: Option<String>,
    pub agent_title: Option<String>,
    pub agent_version: Option<String>,
    pub multi_agent_turns: Vec<common::workspace::threads::MultiAgentTurn>,
    pub subagents: Vec<common::workspace::threads::ThreadAgent>,
}
