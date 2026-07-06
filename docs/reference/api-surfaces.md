# API surfaces reference

The daemon's programmable surfaces: MCP tools for agents, local API routes for model clients, and the WebSocket endpoints. HTTP `/api/*` REST routes are internal contracts consumed by the dashboard and `va-client`; they are not yet a stable public API.

## MCP tools

Served at `/mcp` (JSON-RPC over streamable HTTP, token-authenticated). Auto-injected into enabled agents' global configs when `integrations.mcp_auto_install` is on.

| Tool | Purpose |
|---|---|
| `get_session_id` | Resolve the calling agent session's identity |
| `prepare_handover` | Issue a pickup code (4-character, 120 s TTL, one-shot) for cross-surface continuity |
| `register_workspace` | Register the current project directory as a workspace |
| `initialize_subagents` | Start a multi-agent turn — modes: `parallel`, `collaboration`, `brainstorming` |
| `wait_for_subagents` | Block until subagents report completion; returns their reports |
| `preview` | Create a live preview for a dev server port |
| `md_preview` | Create a rendered Markdown preview |

Companion skills installed per agent (`skill_auto_install`): `vibearound` (handover), `va-session`, `va-preview`, `va-md-preview`, `agent-collaboration`.

## Local API route families

Loopback-only, gated by the local-bridge check; bodies up to 64 MB. Mechanism: [Local API and bridge](../architecture/local-api-and-bridge.md).

```text
/va/local-api/{profile}/{scope}/{api_type}/v1/{responses | chat/completions | messages | models}
/va/local-agent/{agent}/{profile}/v1/{responses | chat/completions | messages | models}
/va/bridge/{profile}/{api_type}/v1/…            (legacy shape)
```

`{api_type}` ∈ `openai-responses` | `openai-chat` | `anthropic` | `gemini`. Gemini clients additionally get the generateContent-shaped route.

### Copy-paste examples

Local-bridge routes need **no Authorization header** — the gate is loopback peer + loopback Host (they are unreachable through tunnels). Substitute your profile id and a model id the profile exposes.

List the models a profile serves:

```bash
curl http://127.0.0.1:12358/va/local-api/moonshot/curl-test/openai-chat/v1/models
```

Chat completion through the bridge (client speaks OpenAI Chat; the daemon translates to whatever the profile's provider speaks):

```bash
curl http://127.0.0.1:12358/va/local-api/moonshot/curl-test/openai-chat/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{"model": "kimi-k2.7-code", "messages": [{"role": "user", "content": "hello"}]}'
```

Agent-as-API — the same request shape, but executed by a hosted coding agent (tools, workspace and all) instead of a bare model:

```bash
curl http://127.0.0.1:12358/va/local-agent/claude/direct/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{"model": "claude", "messages": [{"role": "user", "content": "what does this repo do?"}]}'
```

Add `"stream": true` to either body for SSE streaming. The `{scope}` path segment (`curl-test` above) is free-form launch metadata — anything URL-safe works for manual calls.

## WebSocket endpoints

All token-authenticated; see [architecture overview](../architecture/overview.md#communication-paths) for payload details.

| Endpoint | Purpose |
|---|---|
| `/ws?session_id=` | Terminal bytes + JSON resize (web terminal ↔ PTY) |
| `/ws/chat` | Web/TUI chat events |
| `/ws/channels`, `/ws/tunnels`, `/ws/sessions`, `/ws/agents/runtime` | Live state: full-list snapshot on every change |

## Preview URLs

| URL | Auth | Lifetime |
|---|---|---|
| `/preview/u/{slug}` | Owner token | While the preview exists |
| `/preview/s/{slug}` | None | 600 s |
| `/md-preview/{slug}` | Owner token | While it exists |

---

*Source anchors: `src/server/src/web_server/mcp/mod.rs` (tool dispatch), `src/core/src/workspace/handover.rs` (code TTL), `src/server/src/web_server/api_bridge/routes.rs` + `mod.rs` (route table, body limit), `src/server/src/web_server/ws_domains.rs` (state endpoints), `src/core/src/previews/store.rs` (share TTL).*
*Last verified: v0.7.11*

<sub>[◀ CLI reference](cli.md) · [Documentation index](../README.md) · [Timers and limits ▶](timers-and-limits.md)</sub>
