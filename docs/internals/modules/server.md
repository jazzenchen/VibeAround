# Module: server

`src/server/` — the axum shell over core: HTTP, WebSockets, MCP, the API bridge, previews serving, and daemon assembly. Everything network-facing lives here; everything stateful lives in core.

## Responsibility

Expose core's managers over the wire and own daemon composition: `ServerDaemon::start_background` builds the whole runtime (stores, channel hub, input workers, plugins, search, web server, tunnel) and `RunningDaemon::stop` unwinds it in order.

## Submodules

| Submodule | Role |
|---|---|
| `lib.rs` (`ServerDaemon`, `RunningDaemon`) | Boot sequence, channel input dispatcher, orphan sweep, ingress-first graceful shutdown, Windows bind retry |
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

1. **Route protection layout**: everything is token-gated except the deliberate open set (SPA shell/assets, code-gated Preview Shares, pairing entry). Owner Preview routes require loopback/token access or remote pairing. The Server Share page-preview proxy accepts only GET/HEAD iframe navigations and browser-declared static subresources; browser fetch/XHR/EventSource, workers, non-GET/HEAD methods, WebSockets/HMR, and `/va/*` forwarding stay rejected or unsupported. It is not an API-isolation sandbox, so route policy must not depend on a child path looking like an API. New routes default to protected; adding an open route is a security-model change.
2. **Local-bridge gate on model surfaces**: local-api / local-agent / legacy bridge routes must stay loopback-only and outside the tunnel's reach.
3. **Shutdown order matters** (`RunningDaemon::stop`): stop accepting Web/input → drain `ConversationIngress` → stop channel-owned processes → stop workspace hosts/search → safety-net registry kill → previews/PTY/listeners. A queued prompt must never run after workspace teardown.
4. **`ws_domains` protocol is snapshot-replace** — clients treat the last message as the state; do not introduce incremental diffs on these endpoints (that is what the design rejects to avoid schema drift).
5. Handlers stay thin: parse, call core, serialize. Business rules belong in core.

## Known debt

- `ws_chat.rs` is smaller after parser/event extraction, but session-intent side effects still happen before route-lane ordering and can interleave across WebSocket connections.
- REST handlers + Tauri IPC + va-client + client-ts remain hand-maintained mirrors of one control-plane contract.
- Server unit tests are broad; cross-surface contract fixtures and concurrent lifecycle fault tests remain thin.

---

*Source anchors: `src/server/src/lib.rs`, `src/server/src/web_server/` (all submodules above).*
*Last verified: `codex/im-acp-route-refactor` at `0ba7fa2e` (2026-07-11).*

<sub>[◀ Module: auth](auth.md) · [Documentation index](../../README.md) · [Launch subsystem ▶](../launch.md)</sub>
