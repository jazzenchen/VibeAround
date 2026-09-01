//! HTTP/WebSocket API response shapes for the dashboard.
//!
//! Canonical TypeScript validators live in `src/shared/client-ts/src/schemas.ts`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use common::previews::PreviewSnapshot;
use common::profiles::{catalog, AuthMode};
use common::routing::RouteKey;

/// `GET /api/service/health` response.
#[derive(Debug, Clone, Serialize)]
pub struct ServiceHealthResponse {
    pub ok: bool,
    pub service: &'static str,
    pub version: &'static str,
}

/// `GET /api/service/info` response.
#[derive(Debug, Clone, Serialize)]
pub struct ServiceInfoResponse {
    pub service: &'static str,
    pub version: &'static str,
    pub port: u16,
    pub mode: &'static str,
    pub auth_mode: &'static str,
    pub data_dir: String,
    pub settings_path: String,
    pub web_dist_path: String,
    pub host_search_available: bool,
    pub replace_provider_web_search: bool,
}

/// `PUT /api/settings` response.
#[derive(Debug, Clone, Serialize)]
pub struct SettingsWriteResponse {
    pub ok: bool,
}

/// Per-agent display info returned under `AgentsConfig.agents`.
///
/// # Wire format (JSON)
/// ```json
/// { "id": "claude", "name": "Claude Code", "description": "Claude Code CLI" }
/// ```
///
/// - `id`: an agent ID from the built-in agent registry (e.g. `"claude"`,
///   `"codex"`, `"pi"`, `"gemini"`, `"qwen-code"`).
/// - `name` / `description`: copied from that file's `display_name` and
///   `description` fields.
#[derive(Debug, Clone, Serialize)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub requires_profile: bool,
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
///   agent registry), ordered as configured.
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

/// One user-managed model/API profile without credentials.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProfileSummary {
    pub id: String,
    pub label: String,
    pub provider: String,
    pub provider_label: String,
    pub provider_icon: Option<String>,
    pub auth_mode: AuthMode,
    pub api_types: Vec<String>,
    pub launch_targets: Vec<ModelProfileLaunchTarget>,
    pub api_type_models: BTreeMap<String, String>,
    pub api_type_model_options: BTreeMap<String, Vec<catalog::ModelDef>>,
    pub api_type_headers: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProfileLaunchTarget {
    pub id: String,
    pub label: String,
    pub api_type: String,
}

/// `GET /api/launcher/preferences` response.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherPreferencesResponse {
    pub selected_agent: String,
    pub default_agent: String,
    pub default_profile_id: Option<String>,
    pub enabled_agents: Vec<String>,
    pub agent_preferences: BTreeMap<String, LauncherAgentPreferenceSummary>,
    pub local_agent_api_enabled: bool,
    /// Canonical agent ids opted in to the agent-as-API routes.
    pub local_agent_api_agents: Vec<String>,
    pub profile_connections: common::agent_state::ProfileConnectionPreferences,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherAgentPreferenceSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable_path: Option<String>,
    #[serde(skip_serializing_if = "common::agent_state::AgentLaunchArgs::is_empty")]
    pub launch_args: common::agent_state::AgentLaunchArgs,
}

/// One env assignment in a server-generated launch plan.
#[derive(Debug, Clone, Serialize)]
pub struct LaunchPlanEnvVar {
    pub key: String,
    pub value: String,
}

/// `POST /api/launcher/plan` response.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchPlanResponse {
    pub launch_id: String,
    pub agent_id: String,
    pub profile_id: Option<String>,
    pub launch_target: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<LaunchPlanEnvVar>,
    pub cwd: String,
    pub resume_session_id: Option<String>,
    pub native_execution: bool,
    pub display: LaunchPlanDisplay,
}

#[derive(Debug, Clone, Serialize)]
pub struct LaunchPlanDisplay {
    pub title: String,
}

impl AgentInfo {
    /// Build an `AgentInfo` for each of the given agent IDs by looking up
    /// the corresponding entry in the built-in agent registry. IDs with no matching
    /// entry are silently dropped.
    pub fn for_ids(ids: &[String]) -> Vec<Self> {
        ids.iter()
            .filter_map(|id| {
                let def = common::resources::agent_by_id(id)?;
                Some(Self {
                    id: id.clone(),
                    name: def.display_name.clone(),
                    description: def.description.clone(),
                    requires_profile: def.requires_profile,
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
///   "instance_id": "telegram",
///   "kind": "telegram",
///   "version": "0.1.0",
///   "plugin_dir": "/path/to/va-plugin-channel-telegram",
///   "status": "running",
///   "reason": null
/// }
/// ```
///
/// `status` is one of: `"not_started" | "spawning" | "running" | "crashed" | "stopped"`.
/// `reason` carries a short explanation for crashed/stopped states.
#[derive(Debug, Clone, Serialize)]
pub struct ChannelRuntime {
    pub instance_id: String,
    pub kind: String,
    pub version: Option<String>,
    pub plugin_dir: Option<String>,
    pub status: &'static str,
    pub reason: Option<String>,
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

/// One resumable coding-agent session discovered from a CLI-owned session store.
#[derive(Debug, Clone, Serialize)]
pub struct LaunchSessionInfo {
    pub agent_id: String,
    pub host_agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_profile_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_provider_label: Option<String>,
    pub session_id: String,
    pub title: String,
    pub workspace: String,
    pub updated_at: u64,
    pub short_id: String,
    pub archived: bool,
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
}

/// `POST /api/workspace-threads/init` request.
#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceThreadInitRequest {
    /// Adopt this existing thread and answer with its web identity. The thread
    /// already knows its agent, profile and workspace, so the other fields are
    /// ignored when it is set.
    pub thread_id: Option<String>,
    pub agent_id: Option<String>,
    pub profile_id: Option<String>,
    pub session_id: Option<String>,
    pub workspace_path: Option<String>,
}

/// `POST /api/workspace-threads/init` response.
#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceThreadInitResponse {
    pub thread_id: String,
    pub chat_id: String,
    pub agent_id: String,
    pub profile_id: Option<String>,
    pub session_id: Option<String>,
    pub workspace: String,
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
/// `default_workspace` is the workspace root used for new sessions when no
/// more specific workspace has been selected.
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
/// command_menu / permission_request / turn_status / system_text / error)
/// are VibeAround dashboard metadata. Streaming tokens
/// and tool calls arrive as raw ACP `SessionNotification` payloads under
/// the `acp_notification` kind.
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
/// { "kind": "session_info", "info": { "threadId": "wt_...", "agent": { ... } } }
/// { "kind": "turn_status", "active": false }
/// { "kind": "replay_start", "session_id": "01HX..." }
/// { "kind": "replay_done", "session_id": "01HX..." }
/// { "kind": "preview_refresh" }
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
    /// What the route actually runs now: workspace, thread, agent, profile and
    /// session. The one answer a surface can trust over its own selection.
    SessionInfo {
        info: common::channels::types::ChannelSessionInfo,
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
    TurnStatus {
        active: bool,
    },
    /// Brackets around a session-transcript replay. Everything between the
    /// two markers re-renders history for `session_id`: the client resets its
    /// view of that session on `replay_start` and treats frames until
    /// `replay_done` as the authoritative transcript.
    ReplayStart {
        session_id: String,
    },
    ReplayDone {
        session_id: String,
    },
    PreviewRefresh,
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

impl From<common::workspace::manager::WorkspaceThreadRuntimeEntry> for AgentRuntime {
    fn from(entry: common::workspace::manager::WorkspaceThreadRuntimeEntry) -> Self {
        let st = entry.state;
        let (agent_name, agent_title, agent_version) = st
            .initialize
            .as_ref()
            .and_then(|i| i.agent_info.as_ref())
            .map(|info| {
                (
                    Some(info.name.clone()),
                    info.title.clone(),
                    Some(info.version.clone()),
                )
            })
            .unwrap_or((None, None, None));
        let (channel_kind, chat_id) = match entry.route {
            Some(route) => (route.channel_kind.clone(), route.chat_id.clone()),
            None => ("workspace".to_string(), st.thread_id.to_string()),
        };
        let profile = st.host_binding.profile_id.clone();
        let profile_label = agent_profile_label(profile.as_deref());
        Self {
            thread_id: st.thread_id.to_string(),
            channel_kind,
            chat_id,
            attached_routes: entry.attached_routes.iter().map(Into::into).collect(),
            cli_kind: Some(st.host_binding.agent_id.clone()),
            profile,
            profile_label,
            session_id: st.session_id,
            workspace: Some(st.workspace.to_string_lossy().to_string()),
            busy: st.busy,
            failed: st.failed,
            started_at: 0,
            agent_name,
            agent_title,
            agent_version,
            multi_agent_turns: st.multi_agent_turns,
            subagents: st.agents,
        }
    }
}

/// One route subscribed to a thread, as listed under `attached_routes`.
#[derive(Debug, Clone, Serialize)]
pub struct AgentAttachedRoute {
    pub channel_kind: String,
    pub chat_id: String,
}

impl From<&RouteKey> for AgentAttachedRoute {
    fn from(route: &RouteKey) -> Self {
        Self {
            channel_kind: route.channel_kind.clone(),
            chat_id: route.chat_id.clone(),
        }
    }
}

pub fn agent_profile_label(profile_id: Option<&str>) -> Option<String> {
    common::profiles::load_profile(profile_id?).map(|profile| profile.label)
}

/// One agent runtime, as returned by `GET /api/agents/runtime`.
///
/// Sources: live `ThreadRuntimeState` entries from `WorkspaceThreadManager`.
///
/// # Wire format (JSON)
/// ```json
/// {
///   "thread_id": "wt_0123456789abcdef",
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
    pub thread_id: String,
    pub channel_kind: String,
    pub chat_id: String,
    pub attached_routes: Vec<AgentAttachedRoute>,
    pub cli_kind: Option<String>,
    pub profile: Option<String>,
    pub profile_label: Option<String>,
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
