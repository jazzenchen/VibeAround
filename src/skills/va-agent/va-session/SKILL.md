---
name: va-session
description: "VibeAround Agent only: resolve the current session ID for VibeAround tools. Called by va-preview and vibearound handover when running as the VibeAround Agent."
---

# VibeAround Session ID

Resolve the current session ID. Other VibeAround skills reference this skill when they need session context for preview, handover, or lifecycle management.

## How to Resolve

Call the built-in `get_session_id` tool. It takes no arguments and returns the
current session ID directly; VibeAround already knows this session because it
started this agent.

```
Tool: get_session_id
Arguments: (none)
```

Do not call the `va_mcp_get_session_id` MCP tool for this step — the built-in
tool is the source of truth for the VibeAround Agent.

## Return Value

Return the session ID string to the calling skill.
