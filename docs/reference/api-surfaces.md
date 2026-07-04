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

*Source anchors: `src/server/src/web_server/mcp/mod.rs` (tool dispatch), `src/core/src/workspace/handoff.rs` (code TTL), `src/server/src/web_server/api_bridge/routes.rs` + `mod.rs` (route table, body limit), `src/server/src/web_server/ws_domains.rs` (state endpoints), `src/core/src/previews/store.rs` (share TTL).*
*Last verified: v0.7.11*

<sub>[◀ CLI reference](cli.md) · [Documentation index](../README.md) · [Timers and limits ▶](timers-and-limits.md)</sub>
