# API surfaces reference

The daemon's programmable surfaces: MCP tools for agents, local API routes for model clients, and the WebSocket endpoints. HTTP `/api/*` REST routes are internal contracts consumed by the dashboard and `va-client`; they are not yet a stable public API.

## MCP tools

Served at `/mcp` (JSON-RPC over streamable HTTP). Each daemon start creates an MCP-only credential, stored as `mcp_token` in `~/.vibearound/auth.json`; every VibeAround launch writes the current credential into that agent's project-scoped MCP config. The credential is rejected by dashboard/control APIs.

| Tool | Purpose |
|---|---|
| `va_mcp_get_session_id` | Resolve the calling agent session's identity |
| `va_mcp_prepare_handover` | Issue a pickup code (4-character, 120 s TTL, one-shot) for cross-surface continuity |
| `va_mcp_register_workspace` | Register the current project directory as a workspace |
| `va_mcp_initialize_subagents` | Start a multi-agent turn — modes: `parallel`, `collaboration`, `brainstorming` |
| `va_mcp_wait_for_subagents` | Block until subagents report completion; returns their reports |
| `va_mcp_preview` | Preview exactly one source: a running dev-server `port` or a Markdown `file`. Markdown is rendered directly without starting a separate server |

Every launch also replaces the VibeAround-reserved project skills with the bundled versions: `vibearound` (handover), `va-session`, `va-preview`, and, where supported, `agent-collaboration`.

## Local API route families

Loopback-only and gated by the local-bridge check; the primary `/local-api` and `/local-agent` families also require their own credentials. Bodies up to 64 MB. Mechanism: [Local API and bridge](../architecture/local-api-and-bridge.md).

```text
/va/local-api/{profile}/{scope}/{api_type}/v1/{responses | chat/completions | messages | models}
/va/local-agent/{agent}/{profile}/v1/{responses | chat/completions | messages | models}
/va/bridge/{profile}/{api_type}/v1/…            (legacy shape)
```

`{api_type}` ∈ `openai-responses` | `openai-chat` | `anthropic` | `gemini`. Gemini clients additionally get the generateContent-shaped route.

### Copy-paste examples

Both keys live in `~/.vibearound/auth.json`: set `LOCAL_API_KEY` to `bridge_token` and `LOCAL_AGENT_API_KEY` to `agent_token`. The bridge key rotates on every daemon restart; the agent-as-API key persists across restarts and changes only when you regenerate it from the desktop Local API panel, which also exposes it for copying.

List the models a profile serves:

```bash
curl http://127.0.0.1:12358/va/local-api/moonshot/curl-test/openai-chat/v1/models \
  -H "Authorization: Bearer $LOCAL_API_KEY"
```

Chat completion through the bridge (client speaks OpenAI Chat; the daemon translates to whatever the profile's provider speaks):

```bash
curl http://127.0.0.1:12358/va/local-api/moonshot/curl-test/openai-chat/v1/chat/completions \
  -H "Authorization: Bearer $LOCAL_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"model": "kimi-k2.7-code", "messages": [{"role": "user", "content": "hello"}]}'
```

Agent-as-API — the same request shape, but executed by a hosted coding agent (tools, workspace and all) instead of a bare model:

```bash
curl http://127.0.0.1:12358/va/local-agent/claude/{managed-profile-id}/v1/chat/completions \
  -H "Authorization: Bearer $LOCAL_AGENT_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"model": "claude", "messages": [{"role": "user", "content": "what does this repo do?"}]}'
```

Add `"stream": true` to either body for SSE streaming. The `{scope}` path segment (`curl-test` above) is free-form launch metadata — anything URL-safe works for manual calls.

### Agent-as-API contract

- **Opt-in per agent.** `local_agent_api.enabled` gates the route family; each agent must also be listed in `local_agent_api.agents` (Desktop → Local API → the per-agent switch). A not-opted-in agent answers **403**; the **direct profile is always refused (429)** — use a managed profile.
- **Stateful conversations.** Responses-protocol requests chain `previous_response_id` (bridge-minted; only the latest id is continuable — older ids answer 409, unknown ids 404 after a daemon restart). Chat/messages requests may carry `x-vibearound-conversation: <your-key>` (echoed back) to share one persistent backend session per key: known conversations receive only the new tail of the transcript, new or lost ones are seeded from the full history you send anyway. Keyless chat/messages requests stay sessionless one-shots. A new request on a busy conversation cancels and displaces the in-flight turn. Client-side edits to already-answered history do not take effect — change the key to start over; changed system instructions reseed automatically.
- **Headers.** `x-vibearound-cwd` selects the workspace; `x-vibearound-permission-mode` is forwarded verbatim as the session mode when a fresh backend session is created — use the agent's own mode ids (typically `default` / `plan` / `acceptEdits` / `bypassPermissions`); an unknown id gets the agent's own error. Tool permission prompts over the API are auto-refused, so autonomous use pairs with `acceptEdits` or `bypassPermissions`.
- **Progress.** Tool activity and plan updates stream on the reasoning channel (`reasoning_content` / thinking blocks), so tool-heavy turns show progress instead of a silent stream. The spawn→session startup chain has a 180 s server-side deadline; the turn itself is unbounded — disconnect or send a displacing request to stop it.

## WebSocket endpoints

All token-authenticated; see [architecture overview](../architecture/overview.md#communication-paths) for payload details.

| Endpoint | Purpose |
|---|---|
| `/ws/chat` | Web/TUI chat events |
| `/ws/channels`, `/ws/tunnels`, `/ws/agents/runtime` | Live state: full-list snapshot on every change |

## Preview URLs

| URL | Target | Auth | Lifetime |
|---|---|---|---|
| `/preview/u/{slug}` | Owner shell for Server or Markdown | Loopback or paired owner | While the preview exists |
| `/preview/u/{slug}/content` | Selected owner content; a local Server uses its loopback origin directly | Same owner boundary as the shell | While the preview exists |
| `/preview/s/{share_id}` | Server or Markdown Share | Six-digit access code, then scoped browser grant | One shared 600 s deadline |

The Server Share proxy revalidates the scoped browser grant on every request and forwards authenticated GET/HEAD paths unchanged, including page data reads. Writes, protocol upgrades, service workers, WebSockets, and HMR remain unsupported. `/va/*`, owner pages, chat, and review controls are excluded from a Share. This is a page-preview transport, not general API compatibility or an API-isolation sandbox; accepted GET/HEAD paths are not classified by name.

---

*Source anchors: `src/server/src/web_server/mcp/mod.rs` (tool dispatch), `src/core/src/workspace/handover.rs` (code TTL), `src/server/src/web_server/api_bridge/routes.rs` + `mod.rs` (route table, body limit), `src/server/src/web_server/ws_domains.rs` (state endpoints), `src/core/src/previews/store.rs` (share TTL).*
*Last verified: v0.7.24*

<sub>[◀ CLI reference](cli.md) · [Documentation index](../README.md) · [Timers and limits ▶](timers-and-limits.md)</sub>
