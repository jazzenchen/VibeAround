# How it works

This page follows the two journeys that define VibeAround: an IM message reaching a coding agent, and an agent CLI being launched in your terminal. If a term is unfamiliar, see [Concepts](concepts.md).

## The runtime at a glance

```text
 web SPA ───────────────┐       TUI / CLI (va)          desktop-ui (React)
  │HTTP   │WS ×3        │             │ HTTP + WS            │ Tauri IPC
  │/api/* │/ws (pty)    │             │ (va-client)          ▼
  │       │/ws/chat     │             │                desktop (Tauri shell)
  │       │/ws/* state  │             │                      │ embeds in-process
  ▼       ▼             ▼             ▼                      ▼
   ┌───────────────────────────────────────────────────────────────┐
   │              vibearound-server (axum daemon)                  │
   │  REST /api/* · WS (pty, chat, live state) · MCP /mcp          │
   │  api_bridge /va/local-api · preview reverse proxy · pairing   │
   ├───────────────────────────────────────────────────────────────┤
   │  core runtime                                                 │
   │  channels · workspace threads · process supervisor ·          │
   │  profiles · pty · previews · tunnels · auth · search          │
   └──┬──────────────┬──────────────┬───────────────┬──────────────┘
      │ stdio        │ stdio        │ pseudo-tty    │ child / SDK
      │ JSON-RPC     │ JSON-RPC     ▼               ▼
      │ (ACP)        │ (ACP)    shell / agent   tunnel processes
      ▼              ▼          CLI sessions    (cloudflared, npx
  channel plugin  agent CLI     (web terminal)  localtunnel; ngrok
  processes       processes                     via in-process SDK)
  (telegram, …)      │
                     │ HTTP loopback (bridged profiles)
                     ▼
             /va/local-api bridge ──HTTPS──► upstream model APIs
```

Everything is one process plus supervised children. The desktop app embeds the same daemon the standalone `va serve` runs; every UI is a client of it.

## Communication paths

Every edge in the picture, with its transport and payload shape:

| Edge | Transport | Protocol / payload |
|---|---|---|
| web SPA → server (REST) | HTTP `/api/*` | JSON, bearer token |
| web SPA ↔ server (terminal) | WebSocket `/ws?session_id=` | raw terminal bytes + JSON resize messages |
| web SPA ↔ server (chat) | WebSocket `/ws/chat` | JSON chat events: typed inputs, streamed output, permission cards |
| web SPA ↔ server (live state) | WebSocket `/ws/channels`, `/ws/tunnels`, `/ws/sessions`, `/ws/agents/runtime` | full-list snapshots re-sent on every change ("last message is the state") |
| TUI / `va` CLI → server | HTTP + `/ws/chat` | same contracts, via the `va-client` protocol crate |
| desktop-ui → desktop shell | Tauri IPC | native commands (windows, launch, config screens) |
| desktop shell → daemon | in-process | embeds `ServerDaemon` directly |
| daemon ↔ channel plugins | child stdio | newline-delimited JSON-RPC (ACP framing): envelopes in; outputs, permission cards, `_va/heartbeat` out |
| daemon ↔ agent CLIs | child stdio | ACP (JSON-RPC): initialize, sessions, prompts, notifications, permission requests |
| daemon ↔ web-terminal sessions | pseudo-tty | byte streams through the PTY registry |
| agents → daemon (tools) | HTTP `/mcp` | MCP: JSON-RPC over streamable HTTP (+ SSE) |
| launched CLIs → daemon (models) | HTTP loopback `/va/local-api/…` | the client's provider dialect (OpenAI / Anthropic / Gemini shapes) |
| daemon → model providers | HTTPS | provider dialect after bridge translation |
| daemon → tunnels | in-process SDK (ngrok) or child process (`cloudflared`, `npx localtunnel`) | provider-specific |
| daemon → previewed dev servers | HTTP reverse proxy | pass-through with iframe toolbar injection |

## Module map

Where each responsibility lives. Every runtime module also has a deep-dive page under [modules/](../internals/README.md#modules), and every end-to-end path a walkthrough under [flows/](../internals/README.md#flows):

**`core` — the runtime library (no HTTP server, no UI):**

| Module | Owns |
|---|---|
| `channels` | plugin host, stdio/websocket transports, input dispatch, route lanes, monitor |
| `workspace` | workspaces, threads, route attachments, handover codes (event-sourced state + in-memory pickup codes) |
| `process` | supervisor (spawn/respawn/watchdog), child registry, ACP transport, env enrichment |
| `agent` | ACP agent handle, launch rendering, MCP/skill config injection |
| `profiles` | profile schema, catalog, rendering, bridge launch URLs, provider connections |
| `pty` | PTY session registry and runtime — the web terminal's backend |
| `previews` | live preview registry, owner/share URLs, port cleanup |
| `tunnels` | ngrok / localtunnel / cloudflare providers |
| `auth` | daemon token, pairing codes |
| `launch_sessions` | native CLI session discovery and archiving |
| `plugins` | plugin discovery and manifests |
| `search` | host-side web search runtime |
| `config`, `storage`, `state`, `routing`, `resources` | settings, JSONL event stores, StateSource contract, route keys, agent registry |

**`server` — the axum shell over core:** `web_server/api` (REST), `ws_pty` / `ws_chat` / `ws_domains` (the three WebSocket families), `mcp` (tools endpoint), `api_bridge` (dialect translation, model mapping, content policy, upstream), `preview` (reverse proxy, markdown rendering), `auth` + `pair` (token middleware, pairing), `boot` (daemon assembly).

**Other crates:** `client` (pure Rust protocol library for the HTTP/WS contract), `cli` and `tui` (its consumers), `desktop` (Tauri shell + IPC commands), `launcher` (the `va-launch` native launch binary).

## Journey 1: an IM message becomes an agent reply

1. **Platform to plugin.** The Telegram/Feishu/Slack plugin — a separate Node.js process supervised by the daemon — receives the platform webhook or long-poll event and normalizes it into a channel envelope: route key, message id, text, attachments.

2. **Plugin to daemon.** The envelope crosses into the daemon over the plugin's stdio ACP connection and lands in the channel input queue.

3. **Ordered dispatch.** Inputs are sharded by route key onto worker tasks: messages for the same chat are processed strictly in order, different chats in parallel. This is what keeps rapid-fire messages from racing each other.

4. **Command or prompt.** The text is checked against the slash-command grammar (`/new`, `/close`, `/switch`, `/pickup`, …). Commands are handled by the workspace-thread layer directly; everything else becomes a prompt.

5. **Route to thread.** The route resolves to its attached open thread. First contact on a route creates a thread in a default workspace and attaches the route — no setup step required.

6. **Thread to agent.** The thread ensures its host agent is alive: if needed, the supervisor spawns the agent's ACP adapter in the workspace directory, with the bound profile's environment materialized (credentials, bridge base URLs) and VibeAround's MCP endpoint and skills injected into the agent's config. An existing CLI session is resumed if the thread has one; otherwise a new session is created.

7. **Prompt and stream back.** The prompt goes to the agent over ACP. Notifications stream back — text chunks, tool-call summaries, permission requests — and are fanned out to every route attached to the thread. Permission requests render as interactive cards; the tap comes back as a callback and resolves the agent's pending request.

8. **Keep warm, evict under pressure.** After the turn, the host stays resident for fast reuse; neither Web nor IM starts a fixed idle-shutdown timer. After a genuinely new host starts, an over-limit warm pool may evict at most one eligible least-recently-active host. The thread runtime and CLI session stay intact, so the next message resumes if that host was evicted. See [Timers and limits](../reference/timers-and-limits.md#sizes-and-counts).

## Journey 2: launching an agent CLI in your terminal

Agent Launch is a different path — instead of hosting the agent inside the daemon, VibeAround opens it in your own terminal:

1. You pick an agent, workspace, and model profile (desktop UI or `va launch --profile <name>`).
2. The profile is rendered into a concrete launch: environment variables, per-agent config overlays, and — when the profile targets a bridged provider — base URLs pointing at the daemon's local API (`http://127.0.0.1:12358/va/local-api/…`).
3. The native launcher (`va-launch`, a standalone binary shipped with both the desktop app and the CLI) validates the plan, installs project-scoped MCP/skill integrations if the daemon is running, and opens the agent in your terminal app (Terminal.app, iTerm2, PowerShell, or a Linux terminal).
4. The launched CLI talks to the bridge for models, so a Kimi or DeepSeek subscription can power Codex or Claude Code natively. Sessions created this way are discovered by the daemon and appear as resumable — which is how a terminal session can later be picked up from IM.

## Where state lives

| State | Where | Survives restart? |
|---|---|---|
| Workspaces, threads, route attachments | JSONL event logs in `~/.vibearound/` | Yes |
| Agent CLI sessions (transcripts) | Each agent's own storage | Yes (owned by the agent) |
| Model profiles, settings | `~/.vibearound/settings.json` + profile store | Yes |
| Running agent/plugin processes | In-memory, supervised | No — respawned on demand |
| Auth token for the dashboard | `~/.vibearound/auth.json` | Regenerated each daemon start |

The supervisor gives every child process (channel plugins, agent adapters) crash-respawn with heartbeat watchdogs for plugins, and cascading cleanup on daemon shutdown so no orphan processes outlive it.

---

*Source anchors: `src/server/src/lib.rs` (daemon boot, input sharding), `src/server/src/web_server/mod.rs` + `ws_pty.rs` / `ws_chat.rs` / `ws_domains.rs` (WebSocket families), `src/core/src/lib.rs` (module map), `src/core/src/channels/` (plugin transport, dispatch), `src/core/src/workspace/` (threads, attachments), `src/core/src/process/supervisor.rs` + `acp_transport.rs` (ACP framing), `src/core/src/tunnels/providers/` (tunnel process forms), `src/core/src/profiles/bridge_launch.rs` (local API URLs), `src/launcher/` (va-launch).*
*Last verified: v0.7.11*

<sub>[◀ Concepts](concepts.md) · [Documentation index](../README.md) · [Session lifecycle ▶](session-lifecycle.md)</sub>
