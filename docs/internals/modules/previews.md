# Module: previews

`src/core/src/previews/` — the registry behind Live Preview: which local ports/files are being shared, under which slugs, with which lifetimes.

## Responsibility

Track preview sessions (dev-server ports and rendered files), mint owner and share URLs, enforce the share TTL, and clean up preview-related processes. The HTTP serving side (reverse proxy, iframe toolbar, markdown rendering) lives in [server](server.md)'s `preview` submodule.

## Key types

| Type | File | Role |
|---|---|---|
| Preview store / `SESSIONS` | `store.rs` | Slug → preview session; `SHARE_TTL_SECS = 600` |
| Owner vs share semantics | `store.rs` | Owner URL lives with the preview; share URL expires 600 s after minting |
| `kill_by_session` / `shutdown_kill_all_ports` | `mod.rs` | Kill dev-server processes tied to an agent session / all previewed ports at daemon stop |

## Interactions

- **← server (MCP `preview` / `md_preview`):** agents create previews via tools; skills (`va-preview`, `va-md-preview`) wrap them.
- **← server (`preview/` handlers):** resolve slugs, proxy requests, render markdown.
- **← workspace:** closing a thread kills previews bound to its session.
- **← cli / dashboard:** list and delete.

## Invariants — do not break

1. **Share URLs are the only unauthenticated surface** — single slug, hard TTL. Never widen scope or lifetime without revisiting the [security model](../../architecture/security-model.md).
2. **Preview processes are session-scoped**: an agent session's dev servers die with `/close` and with the daemon — no orphaned `npm run dev`.
3. Owner links require the daemon token; share expiry must not affect the owner path.

## Known debt

- None tracked in the remediation plan.

---

*Source anchors: `src/core/src/previews/` (mod, store), `src/server/src/web_server/preview/` (proxy, iframe, markdown, cookie_proxy), `src/server/src/web_server/mcp/tools.rs` (preview tools).*
*Last verified: v0.7.11*

<sub>[◀ Module: pty](pty.md) · [Documentation index](../../README.md) · [Module: tunnels ▶](tunnels.md)</sub>
