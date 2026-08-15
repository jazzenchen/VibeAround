# Module: previews

`src/core/src/previews/` — the registry behind Live Preview: which local ports/files are being previewed, under which slugs, with which lifetimes.

## Responsibility

Track current-daemon Preview sessions (dev-server ports and Markdown files) in memory, mint Server/Markdown owner and Share identities, enforce the shared access deadline, and clean up registered Server ports. The HTTP side (owner shell, Server routing, Share gate, and direct Markdown rendering without a child static server) lives in [server](server.md)'s `preview` submodule.

## Key types

| Type | File | Role |
|---|---|---|
| Preview store / `SESSIONS` | `store.rs` | Slug → preview session; `SHARE_TTL_SECS = 600` |
| Owner vs Share semantics | `mod.rs`, `store.rs` | Each target has a stable owner slug; its Share ID, code, and grant form one 600 s transaction |
| `delete_session` / `shutdown_kill_all_ports` | `mod.rs` | Kill the current listener when a Server Preview closes / all registered ports at daemon stop |

## Interactions

- **← server (MCP `preview`):** agents pass either a dev-server port or a Markdown file; the `va-preview` skill wraps the unified tool.
- **← server (`preview/` handlers):** resolve slugs, render the owner picker and Markdown content; local Server owners load their loopback origin directly, remote owners use the transparent loopback proxy, and Server Shares use the narrower page-preview proxy.
- **← cli / dashboard:** list and delete.

## Invariants — do not break

1. **Server owner behavior is intentionally small** — before creating a Server iframe, the owner SPA asks for one risk acknowledgement per Preview and browser session. Local owners load the loopback origin directly. Remote owners transparently forward normal HTTP and WebSocket/HMR traffic only to `127.0.0.1:<registered-port>`; `/va/*` remains reserved. Do not add liveness, content, workspace, process, header, or redirect inspection to this path.
2. **Every Share is one scoped transaction** — one Preview, one opaque URL ID, one reusable six-digit code, one browser grant, and one hard TTL. A Server Share forwards authenticated GET/HEAD paths unchanged, including page data reads. Writes, protocol upgrades, service workers, WebSockets, and HMR must remain unsupported; `/va/*`, owner pages, chat, and review stay excluded. It is a page-preview transport, not an API-isolation sandbox; do not infer policy from path names. Never widen target scope or lifetime without revisiting the [security model](../../architecture/security-model.md).
3. **All Preview registrations are in memory**: File and Server registrations exist only in the current daemon run and are never restored at startup. Closing a Server Preview or the daemon kills the process currently listening on that registered port; thread/session close does nothing.
4. Remote Server and Markdown owner links require owner pairing; Share expiry must not affect the owner path.

## Known debt

- None tracked in the remediation plan.

---

*Source anchors: `src/core/src/previews/` (mod, store), `src/server/src/web_server/preview/` (iframe, markdown, access), `src/server/src/web_server/mcp/tools.rs` (preview tools).*
*Last verified: v0.7.24*

<sub>[◀ Module: pty](pty.md) · [Documentation index](../../README.md) · [Module: tunnels ▶](tunnels.md)</sub>
