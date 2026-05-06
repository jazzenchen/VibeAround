---
name: va-preview
description: Exposes a local dev server or HTML file as a live preview via a shareable public URL, enabling browser and mobile device testing. Use after starting a dev server or creating HTML/CSS/JS files, or when the user asks to "preview this", "show me on my phone", "share a preview link", "open in browser", or "mobile preview". Only available when the VibeAround MCP server is connected.
---

# VibeAround Live Preview

Exposes a local dev server or static files as a live preview via a shareable URL, so the user can view the result in their browser or on a mobile device.

## When to Use

- After starting a dev server (`next dev`, `vite`, `python -m http.server`, etc.)
- After creating HTML/CSS/JS files the user should see
- The user asks to "show me", "preview", "let me see it on my phone", or "share a preview link"

**Proactive behavior**: After starting a dev server or creating a web artifact, ask the user if they'd like a preview link (e.g. "Want me to generate a preview link so you can see it on your phone?"). Only call `preview` after the user confirms.

## Prerequisites

The VibeAround MCP server must be connected (server name: `vibearound`). If not available, tell the user to start the VibeAround desktop app.

## Steps

### 1. Ensure the server is listening

- Verify the port is free: `lsof -i :<port>` should return nothing before starting
- Wait for the server's "Listening on..." message before proceeding
- Use `--host 0.0.0.0` when available for broader network compatibility

### 2. Resolve the session ID

Use the `/va-session` skill to get the current session ID.

### 3. Call preview

```
Tool: preview
Server: vibearound
Arguments:
  port: <the port your server is running on>
  cwd: "<current working directory>"
  session_id: "<session_id from step 2>"  (pass if available)
  title: "<short description of what you built>"  (optional)
```

If the workspace is not registered, call `register_workspace` with the `cwd` first, then retry.

### 4. Present BOTH links to the user

The tool returns an Owner link and a Share link. Always show **both** in this format:

```
Preview 已就绪：
- 你的预览: <owner_url>
- 分享链接: <share_url>（10 分钟有效）
```

Or in English:

```
Preview ready:
- Owner: <owner_url>
- Share: <share_url> (expires in 10 min)
```

**Never omit either link.** The owner link is permanent (requires browser pairing). The share link is temporary and needs no auth — ideal for sending to others or testing on mobile.

## Error Handling

- **MCP server not available**: The VibeAround desktop app may not be running.
- **Workspace not registered**: Call `register_workspace` first, then retry.
- **Port in use**: Check with `lsof -i :<port>` and choose a different port.
