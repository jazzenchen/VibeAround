---
description: "Resolve your current session ID for use with other VibeAround tools"
alwaysApply: false
---

# VibeAround Session ID

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
  agent_kind: "cursor"
  cwd: "<current working directory>"
  launch_id: "<value of $VIBEAROUND_LAUNCH_ID if present>"
  profile_id: "<value of $VIBEAROUND_PROFILE_ID if present>"
  channel_kind: "<value of $VIBEAROUND_CHANNEL_KIND if present>"
  chat_id: "<value of $VIBEAROUND_CHAT_ID if present>"
```

Return the session ID string from the MCP tool. If the tool cannot resolve one,
return nothing — callers handle the missing case gracefully.
