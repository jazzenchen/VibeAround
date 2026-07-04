# Module: server

`src/server/` — the axum shell over core: HTTP, WebSockets, MCP, the API bridge, previews serving, and daemon assembly. Everything network-facing lives here; everything stateful lives in core.

## Responsibility

Expose core's managers over the wire and own daemon composition: `ServerDaemon::start_background` builds the whole runtime (stores, channel hub, input workers, plugins, search, web server, tunnel) and `RunningDaemon::stop` unwinds it in order.

## Submodules

| Submodule | Role |
|---|---|
| `lib.rs` (`ServerDaemon`, `RunningDaemon`) | Boot sequence, 64 sharded input workers, orphan sweep, graceful shutdown, Windows bind retry |
| `web_server/mod.rs` | Router assembly: protected vs open routes, body limits, SPA fallback |
| `web_server/api/` | REST handlers per domain (sessions, workspaces, profiles, launcher, previews, settings, files, runtime) |
| `ws_pty` / `ws_chat` / `ws_domains` | The three WebSocket families: terminal bytes, chat events, live-state snapshots |
| `mcp/` | JSON-RPC dispatch for the 7 MCP tools + session identity |
| `api_bridge/` | Dialect translation pipeline ([bridge request flow](../flows/bridge-request.md)) |
| `preview/` | Reverse proxy, iframe toolbar, markdown rendering, cookie handling |
| `auth.rs` / `pair.rs` | Token middleware (header or `?token=`), local-origin rules, pairing HTTP flow |
| `bridge_recording.rs` | In-memory request/response capture for the launch popup |
| `api_types.rs` | Wire types shared with `va-client` |

## Interactions

- **→ core:** every handler resolves through a core manager (`ChannelManager`, `WorkspaceThreadManager`, `PtySessionManager`, `TunnelManager`, previews, profiles).
- **← all frontends:** web SPA, desktop-ui (via HTTP where used), TUI/CLI via `va-client`.
- **← agents:** MCP calls and local-api model traffic loop back in.
- **desktop:** embeds `ServerDaemon` in-process; the standalone binary and `va serve` use the same type.

## Invariants — do not break

1. **Route protection layout**: everything is token-gated except the deliberate open set (SPA shell/assets, share previews, pairing entry). New routes default to protected; adding an open route is a security-model change.
2. **Local-bridge gate on model surfaces**: local-api / local-agent / legacy bridge routes must stay loopback-only and outside the tunnel's reach.
3. **Shutdown order matters** (`RunningDaemon::stop`): threads → channel hub → search → `kill_all` → previews → PTYs → listeners with timeout. New subsystems must slot into this sequence, not bolt on.
4. **`ws_domains` protocol is snapshot-replace** — clients treat the last message as the state; do not introduce incremental diffs on these endpoints (that is what the design rejects to avoid schema drift).
5. Handlers stay thin: parse, call core, serialize. Business rules belong in core.

## Known debt

- `ws_chat.rs` (1.7k lines) mixes codec with session-intent side effects that bypass queue ordering — remediation M6.
- REST handlers + Tauri IPC + va-client + client-ts are four hand-maintained mirrors of one contract — remediation H3 (desktop → HTTP, schemars type generation).
- Server-side test density is thin relative to core — tracked as remediation L11.

---

*Source anchors: `src/server/src/lib.rs`, `src/server/src/web_server/` (all submodules above), `reports/architecture-review-remediation-2026-07-04.md` (M6, H3, L11).*
*Last verified: v0.7.11*

<sub>[◀ Module: auth](auth.md) · [Documentation index](../../README.md)</sub>
