---
name: va-session
description: Resolves the current VibeAround session ID for use by other skills. Use when another skill (va-preview, vibearound handover) needs session context, or when the user asks "what is my session ID", "get session info", or "check session status".
---

# VibeAround Session ID

Resolves the current session ID. Other VibeAround skills call this when they need session context for preview, handover, or lifecycle management.

## How to Resolve

If your agent has a built-in session ID tool, call it and return its result —
VibeAround Agent exposes `get_session_id`, which takes no arguments. Otherwise
call the `va_mcp_get_session_id` MCP tool. Do not inspect MCP resources or
resource templates for this step; the session ID is only exposed as a tool.
Include only optional arguments whose values are present:

Read these values if available:

- Current working directory
- `$VIBEAROUND_AGENT_KIND` or `$VIBEAROUND_LAUNCH_TARGET`
- `$VIBEAROUND_LAUNCH_ID`
- `$VIBEAROUND_PROFILE_ID`
- `$VIBEAROUND_CHANNEL_KIND`
- `$VIBEAROUND_CHAT_ID`

```
Tool: va_mcp_get_session_id
Server: vibearound
Arguments:
  agent_kind: "<your agent type, e.g. claude, codex, gemini; prefer $VIBEAROUND_AGENT_KIND or $VIBEAROUND_LAUNCH_TARGET when present>"
  cwd: "<current working directory>"
  launch_id: "<value of $VIBEAROUND_LAUNCH_ID if present>"
  profile_id: "<value of $VIBEAROUND_PROFILE_ID if present>"
  channel_kind: "<value of $VIBEAROUND_CHANNEL_KIND if present>"
  chat_id: "<value of $VIBEAROUND_CHAT_ID if present>"
```

The MCP tool resolves the session from explicit parameters, VibeAround route
state, agent metadata, or workspace-aware auto-discovery, and records it
against the launch context when `launch_id` is available.

## Return Value

Return the session ID string to the calling skill. If neither method succeeds, return nothing — callers handle the missing case gracefully.
