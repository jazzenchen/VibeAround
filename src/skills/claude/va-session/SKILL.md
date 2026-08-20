---
name: va-session
description: Resolve your current session ID for use with other VibeAround tools. Called by other skills that need session context (e.g. va-preview, vibearound handover).
---

# VibeAround Session ID

Resolve your current session ID. Other VibeAround skills reference this skill when they need session context for lifecycle management.

## How to Resolve

Read these values if available:

- Current working directory
- `$VIBEAROUND_LAUNCH_ID`
- `$VIBEAROUND_PROFILE_ID`
- `$VIBEAROUND_CHANNEL_KIND`
- `$VIBEAROUND_CHAT_ID`

For Claude Code and Claude Desktop, use this rendered value as the current
session ID:

```
${CLAUDE_SESSION_ID}
```

Call the `va_mcp_get_session_id` MCP tool. Include only optional arguments whose
values are present. If the rendered Claude value is empty or is still the
literal placeholder text, omit `session_id`.

```
Tool: va_mcp_get_session_id
Server: vibearound
Arguments:
  agent_kind: "claude"
  session_id: "<rendered ${CLAUDE_SESSION_ID} if valid>"
  cwd: "<current working directory>"
  launch_id: "<value of $VIBEAROUND_LAUNCH_ID if present>"
  profile_id: "<value of $VIBEAROUND_PROFILE_ID if present>"
  channel_kind: "<value of $VIBEAROUND_CHANNEL_KIND if present>"
  chat_id: "<value of $VIBEAROUND_CHAT_ID if present>"
```

## Return Value

Return the session ID string from the MCP tool. If the tool cannot resolve one,
return nothing — callers handle the missing case gracefully.
