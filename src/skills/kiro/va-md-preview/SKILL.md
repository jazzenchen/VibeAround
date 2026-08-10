---
inclusion: always
name: va-md-preview
description: Preview a markdown file with beautiful GitHub-style rendering. Use after creating or updating markdown documents.
---

# VibeAround Markdown Preview

After you create or update a markdown document, generate a styled preview so the user can read it in their browser or phone with beautiful formatting.

## Rendering and privacy

The parser is bundled with VibeAround. Raw HTML is shown as source text, and only absolute HTTPS Markdown image URLs are loaded. Image hosts can see the viewer's IP address; Preview sends no referrer.

## When to Use

- You just created or updated a README.md, documentation, or any .md file
- The user asks to "show me the doc", "preview the README", or "let me see it"
- Only when the VibeAround MCP server is connected

**Proactive behavior**: After creating or updating any markdown file, proactively ask the user if they'd like to preview it (e.g. "Want me to generate a preview link so you can see it?"). If the user confirms, call `md_preview`. Do NOT call the tool without asking first.

## Prerequisites

The VibeAround MCP server must be connected (server name: `vibearound`). If not available, tell the user to start the VibeAround desktop app.

## Steps

### 1. Resolve the conversation identity

Read `$VIBEAROUND_THREAD_ID` when present. Also use the `va-session` skill to resolve the current session ID. Prefer `thread_id`; otherwise pass both `agent_kind` and `session_id` so the Preview child is linked to the current task.

### 2. Call md_preview

```
Tool: md_preview
Server: vibearound
Arguments:
  file: "<path to the markdown file>"  (absolute or relative to cwd)
  cwd: "<current working directory>"
  thread_id: "<value of $VIBEAROUND_THREAD_ID if present>"
  agent_kind: "kiro"  (pass with session_id when thread_id is unavailable)
  session_id: "<session_id from va-session>"  (pass if available)
  title: "<document title>"  (optional, defaults to filename)
```

If the tool says the workspace is not registered, call `register_workspace` with the `cwd` first, then retry.

### 3. Relay the returned access details

Always show the Owner URL. When the tool also returns a Share URL, present that URL together with the six-digit access code and exact remaining lifetime. State that the code can be reused by multiple viewers until the Share URL and code expire together. If public sharing is unavailable, show only the Owner URL and that message; do not invent a localhost Share URL.

## Error Handling

- **MCP server not available**: The VibeAround desktop app may not be running.
- **Workspace not registered**: Call `register_workspace` first, then retry.
- **File not found**: Verify the file path is correct and the file exists.
