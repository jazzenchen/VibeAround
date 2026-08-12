---
description: Start a live Preview for a running development server. Use after starting a dev server or when the user asks to preview a browsable artifact.
alwaysApply: false
---

# VibeAround Live Preview

Register a running development server so the user can inspect it through VibeAround Preview.

## When to Use

- You started a dev server such as Next.js, Vite, or `python -m http.server`
- You created a browsable artifact and are serving it locally
- The user asked to preview or open the result
- The VibeAround MCP server is connected

**Proactive behavior**: Ask before calling `preview`. Do not call the tool without the user's confirmation.

## Prerequisites

The VibeAround desktop app and MCP server must be running.

## Steps

### 1. Reuse or start the server

- Reuse the actual port when this task already started the intended server and its tracked process is still running
- Do not adopt or kill an arbitrary listener just because it occupies an old port
- Otherwise prefer the framework's automatic port selection; when it supports port `0`, let the OS allocate a port and read the actual port from the startup output
- Respect a project-required fixed port, but do not hardcode a temporary Preview port or maintain a dedicated port range
- Wait until the server reports that it is listening, and keep it on a loopback interface when possible

### 2. Resolve the conversation identity

Pass `$VIBEAROUND_THREAD_ID` when present. If an exact current session ID is readily available, pass it with `agent_kind: "cursor"` for earlier lifecycle cleanup. Both identities are optional: do not delay or block Preview when either is unavailable; VibeAround creates a standalone Preview conversation.

### 3. Call preview

```
Tool: preview
Server: vibearound
Arguments:
  port: <the local server port>
  cwd: "<current working directory>"
  thread_id: "<value of $VIBEAROUND_THREAD_ID if present>"
  agent_kind: "cursor"  (pass with session_id when available)
  session_id: "<exact current session ID>"  (pass if available)
  title: "<short description>"  (optional)
```

If the workspace is not registered, call `register_workspace` with `cwd`, then retry.

### 4. Relay the returned link

Present the Local owner URL and, when returned, the Tunnel owner URL. Do not construct URLs yourself. Server Preview does not create a public Share URL; the Tunnel URL is an owner link and may require pairing.

## Error Handling

- **MCP server unavailable**: Ask the user to start the VibeAround desktop app.
- **Workspace not registered**: Register it, then retry.
- **Server unavailable**: Verify that the reported port is listening.
