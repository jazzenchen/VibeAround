# Module: previews

`src/core/src/previews/` — the registry behind Live Preview: which local ports/files are being previewed, under which slugs, with which lifetimes.

## Responsibility

Track preview sessions (dev-server ports and rendered files), mint Server/Markdown owner and Share identities, enforce the shared access deadline, and clean up preview-related processes. The HTTP side (owner shell, Server routing, Share gate, and Markdown rendering) lives in [server](server.md)'s `preview` submodule.

## Key types

| Type | File | Role |
|---|---|---|
| Preview store / `SESSIONS` | `store.rs` | Slug → preview session; `SHARE_TTL_SECS = 600` |
| Owner vs Share semantics | `mod.rs`, `store.rs` | Each target has a stable owner slug; its Share ID, code, and grant form one 600 s transaction |
| `kill_by_session` / `shutdown_kill_all_ports` | `mod.rs` | Kill dev-server processes tied to an agent session / all previewed ports at daemon stop |

## Interactions

- **← server (MCP `preview` / `md_preview`):** agents create previews via tools; skills (`va-preview`, `va-md-preview`) wrap them.
- **← server (`preview/` handlers):** resolve slugs, render the owner picker and Markdown content; local owners load Server origins directly, while Server Shares use the page-preview proxy.
- **← workspace:** closing a thread kills previews bound to its session.
- **← cli / dashboard:** list and delete.

## Invariants — do not break

1. **Every Share is one scoped transaction** — one Preview, one opaque URL ID, one reusable six-digit code, one browser grant, and one hard TTL. A Server Share forwards only GET/HEAD iframe navigations and browser-declared static subresources; browser fetch/XHR/EventSource, workers, non-GET/HEAD methods, WebSockets, HMR, and `/va/*` forwarding must remain rejected or unsupported. It is a page-preview transport, not an API-isolation sandbox; do not infer policy from path names. Never widen target scope or lifetime without revisiting the [security model](../../architecture/security-model.md).
2. **Preview processes are session-scoped**: an agent session's dev servers die with `/close` and with the daemon — no orphaned `npm run dev`.
3. Remote Server and Markdown owner links require owner pairing; Share expiry must not affect the owner path.

## Known debt

- None tracked in the remediation plan.

---

*Source anchors: `src/core/src/previews/` (mod, store), `src/server/src/web_server/preview/` (iframe, markdown, access), `src/server/src/web_server/mcp/tools.rs` (preview tools).*
*Last verified: v0.7.24*

<sub>[◀ Module: pty](pty.md) · [Documentation index](../../README.md) · [Module: tunnels ▶](tunnels.md)</sub>
