---
name: va-session
description: Resolve your current session ID for use with other VibeAround tools. Called by other skills that need session context (e.g. va-preview, vibearound handover).
---

# VibeAround Session ID

Resolve your current session ID. Other VibeAround skills reference this skill when they need session context for lifecycle management.

## How to Resolve

Call the `va_mcp_get_session_id` MCP tool. Include only optional arguments whose
values are present:

Read these values if available:

- Current working directory
- `$VIBEAROUND_LAUNCH_ID`
- `$VIBEAROUND_PROFILE_ID`
- `$VIBEAROUND_CHANNEL_KIND`
- `$VIBEAROUND_CHAT_ID`

```
Tool: va_mcp_get_session_id
Server: vibearound
Arguments:
  agent_kind: "gemini"
  cwd: "<current working directory>"
  launch_id: "<value of $VIBEAROUND_LAUNCH_ID if present>"
  profile_id: "<value of $VIBEAROUND_PROFILE_ID if present>"
  channel_kind: "<value of $VIBEAROUND_CHANNEL_KIND if present>"
  chat_id: "<value of $VIBEAROUND_CHAT_ID if present>"
```

The MCP tool resolves the session from VibeAround route state or
workspace-aware auto-discovery, and records it against the launch context when
`launch_id` is available.

## Return Value

Return the session ID string to the calling skill. If neither method succeeds, return nothing — callers handle the missing case gracefully.
