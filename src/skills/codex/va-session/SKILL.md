---
name: va-session
description: "Codex only: resolve the current Codex session ID for VibeAround tools. Called by va-preview and vibearound handover when running in Codex."
---

# VibeAround Session ID

Resolve the current Codex session ID. Other VibeAround Codex skills reference this skill when they need session context for lifecycle management.

## How to Resolve

Read these values if available:

- Current working directory
- `$VIBEAROUND_LAUNCH_ID`
- `$VIBEAROUND_PROFILE_ID`
- `$VIBEAROUND_CHANNEL_KIND`
- `$VIBEAROUND_CHAT_ID`

### Method 1: Via Codex MCP metadata (preferred)

Call the `va_mcp_get_session_id` MCP tool with `agent_kind` set to `codex`. Include
only optional arguments whose values are present:

Do not inspect MCP resources or resource templates for this step. VibeAround
exposes `va_mcp_get_session_id` as a tool; Codex's `/mcp` command can be used only for
human diagnostics.

```
Tool: va_mcp_get_session_id
Server: vibearound
Arguments:
  agent_kind: "codex"
  cwd: "<current working directory>"
  launch_id: "<value of $VIBEAROUND_LAUNCH_ID if present>"
  profile_id: "<value of $VIBEAROUND_PROFILE_ID if present>"
  channel_kind: "<value of $VIBEAROUND_CHANNEL_KIND if present>"
  chat_id: "<value of $VIBEAROUND_CHAT_ID if present>"
```

VibeAround reads Codex's MCP call metadata and returns the current Codex
thread/session ID and records it against the launch context when `launch_id`
is available.

## Return Value

Return the session ID string to the calling skill. If neither method succeeds, return nothing — callers handle the missing case gracefully.
