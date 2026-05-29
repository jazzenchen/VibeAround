/**
 * zod schemas for everything the dashboard server returns over HTTP
 * or WebSocket.
 *
 * The source of truth is Rust: look at the `#[derive(Serialize)]` types
 * in `src/server/src/api_types.rs` and the domain types they reference
 * (e.g. `common::tunnels::TunnelStatus`). The docstrings on those Rust
 * types carry JSON examples; this file mirrors them. When the Rust
 * side changes, update the matching schema here in the same PR.
 *
 * Usage: call `.parse()` on every wire-crossing value so bad payloads
 * fail fast at the boundary instead of rotting through the UI.
 */

import { z } from "zod";

// ---------------------------------------------------------------------------
// Agent IDs (mirrors resources/agents.json — order not significant)
// ---------------------------------------------------------------------------

/** Every agent ID defined in `resources/agents.json`. Hand-maintained.
 *  When that file adds an entry, add it here too and the `Record<AgentId, ...>`
 *  consumers (display-name maps) will force you to supply the rest. */
export const AGENT_IDS = [
  "claude",
  "gemini",
  "opencode",
  "codex",
  "pi",
  "cursor",
  "kiro",
  "qwen-code",
] as const;

export type AgentId = (typeof AGENT_IDS)[number];

export const AgentIdSchema = z.enum(AGENT_IDS);

// ---------------------------------------------------------------------------
// Constants mirrored from Rust
// ---------------------------------------------------------------------------

/** Mirror of `common::previews::SHARE_TTL_SECS`. */
export const PREVIEW_SHARE_TTL_SECS = 600;

// ---------------------------------------------------------------------------
// GET /api/agents — enabled agent list + default
// ---------------------------------------------------------------------------

export const AgentInfoSchema = z.object({
  id: z.string(),
  name: z.string(),
  description: z.string(),
});
export type AgentInfo = z.infer<typeof AgentInfoSchema>;

export const AgentsConfigSchema = z.object({
  agents: z.array(AgentInfoSchema),
  default_agent: z.string(),
});
export type AgentsConfig = z.infer<typeof AgentsConfigSchema>;

export const ProfileLaunchTargetSchema = z.object({
  id: z.string(),
  label: z.string(),
  api_type: z.string(),
  bridge_target_api_type: z.string().nullable().optional(),
});
export type ProfileLaunchTarget = z.infer<typeof ProfileLaunchTargetSchema>;

export const ProfileLaunchOptionSchema = z.object({
  id: z.string(),
  label: z.string(),
  provider: z.string(),
  launch_targets: z.array(ProfileLaunchTargetSchema),
});
export type ProfileLaunchOption = z.infer<typeof ProfileLaunchOptionSchema>;
export const ProfileLaunchOptionsSchema = z.array(ProfileLaunchOptionSchema);

// ---------------------------------------------------------------------------
// PTY sessions and workspace/previews endpoints.
// ---------------------------------------------------------------------------

export const PtyToolSchema = z.enum([
  "generic",
  "claude",
  "codex",
  "pi",
  "gemini",
  "opencode",
  "cursor",
  "kiro",
  "qwen-code",
]);
export type PtyTool = z.infer<typeof PtyToolSchema>;

export const PtyRunStateSchema = z.discriminatedUnion("type", [
  z.object({ type: z.literal("running"), tool: PtyToolSchema }),
  z.object({
    type: z.literal("exited"),
    tool: PtyToolSchema,
    exit_code: z.number(),
  }),
]);
export type PtyRunState = z.infer<typeof PtyRunStateSchema>;

export const SessionListItemSchema = z.object({
  session_id: z.string(),
  tool: PtyToolSchema,
  status: PtyRunStateSchema,
  created_at: z.number(),
  project_path: z.string().nullable(),
  profile_id: z.string().nullable(),
  profile_label: z.string().nullable(),
  launch_target: z.string().nullable(),
  tmux_session: z.string().nullable(),
});
export type SessionListItem = z.infer<typeof SessionListItemSchema>;
export const SessionListSchema = z.array(SessionListItemSchema);

export const CreateSessionResponseSchema = z.object({
  session_id: z.string(),
  tool: PtyToolSchema,
  created_at: z.number(),
  project_path: z.string().nullable(),
  profile_id: z.string().nullable(),
  profile_label: z.string().nullable(),
  launch_target: z.string().nullable(),
});
export type CreateSessionResponse = z.infer<typeof CreateSessionResponseSchema>;

export const LaunchSessionInfoSchema = z.object({
  agent_id: z.string(),
  session_id: z.string(),
  title: z.string(),
  workspace: z.string(),
  updated_at: z.number(),
  short_id: z.string(),
  archived: z.boolean(),
  active: z.boolean().optional(),
});
export type LaunchSessionInfo = z.infer<typeof LaunchSessionInfoSchema>;
export const LaunchSessionListSchema = z.array(LaunchSessionInfoSchema);

export const TmuxSessionsResponseSchema = z.object({
  available: z.boolean(),
  sessions: z.array(z.string()),
});
export type TmuxSessionsResponse = z.infer<typeof TmuxSessionsResponseSchema>;

export const WorkspaceItemSchema = z.object({
  path: z.string(),
  is_default: z.boolean(),
  is_builtin: z.boolean(),
});
export type WorkspaceItem = z.infer<typeof WorkspaceItemSchema>;

export const WorkspacesResponseSchema = z.object({
  workspaces: z.array(WorkspaceItemSchema),
  default_workspace: z.string(),
});
export type WorkspacesResponse = z.infer<typeof WorkspacesResponseSchema>;

export const WebVerboseSettingsSchema = z.object({
  show_thinking: z.boolean(),
  show_tool_use: z.boolean(),
  show_archived: z.boolean(),
  send_with_modifier_enter: z.boolean(),
});
export type WebVerboseSettings = z.infer<typeof WebVerboseSettingsSchema>;

export const ChatUploadResponseSchema = z.object({
  id: z.string(),
  name: z.string(),
  mime_type: z.string(),
  size: z.number(),
  uri: z.string(),
});
export type ChatUploadResponse = z.infer<typeof ChatUploadResponseSchema>;

export const PreviewSnapshotSchema = z.object({
  slug: z.string(),
  id: z.string(),
  workspace: z.string(),
  title: z.string(),
  kind: z.enum(["server", "file"]),
  port: z.number().nullable(),
  share_key: z.string().nullable(),
  share_expires_at_ms: z.number().nullable(),
  created_at_ms: z.number(),
});
export type PreviewSnapshot = z.infer<typeof PreviewSnapshotSchema>;

export const PreviewsResponseSchema = z.object({
  previews: z.array(PreviewSnapshotSchema),
  tunnel_url: z.string().nullable(),
});
export type PreviewsResponse = z.infer<typeof PreviewsResponseSchema>;

// ---------------------------------------------------------------------------
// Tunnel status — discriminated union matching Rust `TunnelStatus`
// (`src/core/src/tunnels/status.rs`).
// ---------------------------------------------------------------------------

export const TunnelStatusSchema = z.discriminatedUnion("state", [
  z.object({ state: z.literal("running") }),
  z.object({ state: z.literal("stopped"), reason: z.string() }),
  z.object({ state: z.literal("failed"), error: z.string() }),
]);
export type TunnelStatus = z.infer<typeof TunnelStatusSchema>;

// ---------------------------------------------------------------------------
// Per-domain runtime endpoints.
//
// Reference Rust shape lives in `src/server/src/api_types.rs`. Each
// endpoint:
// - HTTP GET returns the `Array<...>` body.
// - WS /ws/<domain> pushes the full array whenever the kernel manager
//   reports a change.
// ---------------------------------------------------------------------------

/** Channel lifecycle states from `ChannelMonitor`. */
export const CHANNEL_STATUS_VALUES = [
  "not_started",
  "spawning",
  "running",
  "crashed",
  "stopped",
] as const;
export const ChannelStatusSchema = z.enum(CHANNEL_STATUS_VALUES);
export type ChannelStatus = z.infer<typeof ChannelStatusSchema>;

export const ChannelRuntimeSchema = z.object({
  kind: z.string(),
  status: ChannelStatusSchema,
  reason: z.string().nullable(),
  crash_count: z.number(),
  last_seen_age_secs: z.number(),
  restart_in_secs: z.number(),
  started_at: z.number(),
});
export type ChannelRuntime = z.infer<typeof ChannelRuntimeSchema>;
export const ChannelRuntimeListSchema = z.array(ChannelRuntimeSchema);

export const TunnelRuntimeSchema = z.object({
  provider: z.string(),
  url: z.string().nullable(),
  status: TunnelStatusSchema,
  uptime_secs: z.number(),
});
export type TunnelRuntime = z.infer<typeof TunnelRuntimeSchema>;
export const TunnelRuntimeListSchema = z.array(TunnelRuntimeSchema);

export const MultiAgentTurnModeSchema = z.enum([
  "parallel",
  "collaboration",
  "brainstorming",
]);
export type MultiAgentTurnMode = z.infer<typeof MultiAgentTurnModeSchema>;

export const ThreadAgentStatusSchema = z.enum([
  "ready",
  "running",
  "completed",
  "error",
]);
export type ThreadAgentStatus = z.infer<typeof ThreadAgentStatusSchema>;

export const MultiAgentTurnSchema = z.object({
  id: z.string(),
  mode: MultiAgentTurnModeSchema,
  status: ThreadAgentStatusSchema,
  agents: z.array(z.string()),
  created_at: z.string(),
  updated_at: z.string(),
});
export type MultiAgentTurn = z.infer<typeof MultiAgentTurnSchema>;

export const ThreadAgentSchema = z.object({
  id: z.string(),
  turn_id: z.string(),
  name: z.string(),
  agent_id: z.string(),
  profile_id: z.string().optional(),
  session_id: z.string().optional(),
  status: ThreadAgentStatusSchema,
  branch: z.string(),
  worktree: z.string(),
  task: z.string().optional(),
  last_error: z.string().optional(),
  report: z.unknown().optional(),
  created_at: z.string(),
  updated_at: z.string(),
});
export type ThreadAgent = z.infer<typeof ThreadAgentSchema>;

export const AgentRuntimeSchema = z.object({
  route_key: z.string(),
  channel_kind: z.string(),
  chat_id: z.string(),
  cli_kind: z.string().nullable(),
  profile: z.string().nullable(),
  session_id: z.string().nullable(),
  workspace: z.string().nullable(),
  busy: z.boolean(),
  failed: z.string().nullable(),
  started_at: z.number(),
  agent_name: z.string().nullable(),
  agent_title: z.string().nullable(),
  agent_version: z.string().nullable(),
  multi_agent_turns: z.array(MultiAgentTurnSchema),
  subagents: z.array(ThreadAgentSchema),
});
export type AgentRuntime = z.infer<typeof AgentRuntimeSchema>;
export const AgentRuntimeListSchema = z.array(AgentRuntimeSchema);

// ---------------------------------------------------------------------------
// /ws/chat — ChatEvent envelope
//
// Lifecycle events have hand-curated fields; streaming agent output
// rides through `acp_notification` carrying a raw ACP
// `SessionNotification` (from `@agentclientprotocol/sdk`). Consumers
// do a two-level switch: first on the envelope `kind`, then — inside
// `acp_notification` — on `payload.update.sessionUpdate`.
//
// The ACP payload itself isn't re-validated here (we trust the
// agent-client-protocol crate on the server side). If you need
// typed access to specific update variants on the TS side, import
// them from `@agentclientprotocol/sdk` directly.
// ---------------------------------------------------------------------------

export const ChatEventSchema = z.discriminatedUnion("kind", [
  z.object({
    kind: z.literal("config"),
    channel_id: z.string(),
    agents: z.array(AgentInfoSchema),
    default_agent: z.string(),
  }),
  z.object({
    kind: z.literal("agent_ready"),
    agent: z.string(),
    version: z.string(),
  }),
  z.object({
    kind: z.literal("session_ready"),
    session_id: z.string(),
  }),
  z.object({
    kind: z.literal("session_mode"),
    session_mode: z.unknown(),
  }),
  z.object({
    kind: z.literal("command_menu"),
    system_commands: z.unknown(),
    agent_commands: z.unknown(),
  }),
  z.object({
    kind: z.literal("permission_request"),
    request_id: z.string(),
    request: z.unknown(),
  }),
  z.object({
    kind: z.literal("multi_agent_turn"),
    turn: MultiAgentTurnSchema,
    agents: z.array(ThreadAgentSchema),
  }),
  z.object({
    kind: z.literal("subagent_status"),
    agent: ThreadAgentSchema,
  }),
  z.object({
    kind: z.literal("subagent_acp_notification"),
    agent: ThreadAgentSchema,
    payload: z.unknown(),
  }),
  z.object({
    kind: z.literal("prompt_done"),
    message_id: z.string().optional(),
  }),
  z.object({
    kind: z.literal("turn_status"),
    active: z.boolean(),
  }),
  z.object({
    kind: z.literal("system_text"),
    text: z.string(),
  }),
  z.object({
    kind: z.literal("acp_notification"),
    payload: z.unknown(),
  }),
  z.object({
    kind: z.literal("error"),
    error: z.string(),
  }),
]);
export type ChatEvent = z.infer<typeof ChatEventSchema>;
