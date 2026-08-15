---
name: va-preview
description: Start a VibeAround Preview for either a running local web server or a Markdown file. Use after starting a dev server, after creating or updating Markdown, or when the user asks to preview a browsable artifact. Only available when the VibeAround MCP server is connected.
---

# VibeAround Preview

Preview exactly one local source through VibeAround: a running web server or a Markdown file.

## Workflow

### 1. Prepare one source

- **Server**: Reuse the intended server when it is still running. Otherwise start it with the framework's automatic port selection and wait until it is reachable at `127.0.0.1:<port>`. VibeAround registers that port in memory; closing that Preview or the daemon kills the process currently listening there.
- **Markdown**: Verify that the requested file exists. No separate static-file server is needed.

File and Server registrations exist only for the current daemon run. VibeAround never persists or restores them.

### 2. Add optional conversation identity

Pass `$VIBEAROUND_THREAD_ID` when present. If an exact current session ID is readily available, pass it with `agent_kind: "qwen-code"`. Both are optional; do not delay or block Preview when either is unavailable.
These fields only let Preview chat inherit the current task as context. They never control Preview or server lifetime.

### 3. Call `preview`

Pass exactly one of `port` or `file`, plus `cwd`.

For a server:

```
Tool: preview
Server: vibearound
Arguments:
  port: <local server port>
  cwd: "<current working directory>"
  thread_id: "<value of $VIBEAROUND_THREAD_ID if present>"
  agent_kind: "qwen-code"  (pass with session_id)
  session_id: "<exact current session ID if available>"
  title: "<short title>"  (optional)
```

For Markdown:

```
Tool: preview
Server: vibearound
Arguments:
  file: "<absolute path, or path relative to cwd>"
  cwd: "<current working directory>"
  thread_id: "<value of $VIBEAROUND_THREAD_ID if present>"
  agent_kind: "qwen-code"  (pass with session_id)
  session_id: "<exact current session ID if available>"
  title: "<document title>"  (optional)
```

If the workspace is not registered, call `register_workspace` with `cwd`, then retry.

### 4. Relay returned links

Present every owner and Share URL returned by the tool. Include the six-digit access code and exact remaining lifetime with a Share. The Share URL and code expire together and the code can be reused by multiple viewers until expiry. Do not construct URLs yourself.

## Server consent and transport

Do not add an agent-side risk preflight. Before a Server iframe is created, the Preview page itself asks the user once per Preview and browser session to acknowledge that the local server may contain unknown code.

Remote owner traffic is forwarded only to `127.0.0.1:<registered-port>`, including ordinary HTTP methods, request bodies, and WebSocket/HMR traffic. The public Share remains a narrower read-only view.

## Markdown rendering and privacy

VibeAround renders Markdown internally. Raw HTML is shown as source text, and only absolute HTTPS Markdown images are loaded. Image hosts can see the viewer's IP address; Preview sends no referrer.

## Optional server review bridge

Only when the user asks to review or comment on a server preview, add the exact dev-only `<script>` tag returned by the tool. Do not add it proactively or ship it in a production build.

## Errors

- If the MCP server is unavailable, ask the user to start VibeAround.
- If a server is unavailable, verify that the returned port is listening.
- If a file cannot be previewed, verify that it exists and is readable.
