---
name: va-preview
description: "Codex only: start a local-only live preview from a Codex session. Use after starting a dev server or when the user asks to preview a browsable artifact. Only available when the VibeAround MCP server is connected."
---

# VibeAround Live Preview

Start a local live preview for a running development server so the user can inspect the result in the browser on the same machine.

## When to Use

- You started a dev server such as Next.js, Vite, or `python -m http.server`
- You created a browsable artifact and are serving it locally
- The user asked to preview or open the result
- The VibeAround MCP server is connected

**Proactive behavior**: Ask before calling `preview`. Do not call the tool without the user's confirmation.

## Prerequisites

The VibeAround desktop app and MCP server must be running.

## Steps

### 1. Verify the server

- Confirm the selected port is free before starting
- Wait until the server reports that it is listening
- Keep the server on a loopback interface when possible

### 2. Resolve the session ID

Use the `va-session` skill to resolve the current session ID so VibeAround can clean up the dev server with the session.

### 3. Call preview

```
Tool: preview
Server: vibearound
Arguments:
  port: <the local server port>
  cwd: "<current working directory>"
  session_id: "<session_id from va-session>"  (pass if available)
  title: "<short description>"  (optional)
```

If the workspace is not registered, call `register_workspace` with `cwd`, then retry.

### 4. Relay the returned link

The tool returns one local owner URL. Present that URL and state that live-server previews are local-only. The owner iframe loads the server's loopback origin directly, so the app keeps its native API, WebSocket, and HMR behavior. Do not construct a tunnel or Share URL; use `va-md-preview` when the user needs a public document share.

## Error Handling

- **MCP server unavailable**: Ask the user to start the VibeAround desktop app.
- **Workspace not registered**: Register it, then retry.
- **Server unavailable**: Verify that the reported port is listening.
