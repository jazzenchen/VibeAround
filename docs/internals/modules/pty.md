# Module: pty

`src/core/src/pty/` — pseudo-terminal sessions backing the web terminal. Flow: [PTY terminal](../flows/web-terminal.md).

## Responsibility

Spawn shells or agent CLIs under a portable PTY (via `portable_pty`), keep a registry of live sessions, and bridge their byte streams to whoever attaches. Detach-without-kill is the defining behavior: sessions outlive their viewers, not the daemon.

## Key types

| Type | File | Role |
|---|---|---|
| `Registry` (`new_registry`) | `mod.rs` | DashMap of live sessions by id |
| `PtySessionManager` | `manager` (behind `mod.rs`) | Create / attach / delete sessions; tool resolution (shell vs agent pty command) |
| PTY runtime | `runtime.rs` | Child under a pty: spawn, resize, read/write pumps, `try_wait` polling → run-state events |

## Interactions

- **← server (`ws_pty`):** WebSocket attach/detach, resize messages, byte relay.
- **← cli:** `va session create/attach/kill`, `va tmux sessions`.
- **→ process::env:** enriched login-shell environment; `resources::PTY_ENV` terminal defaults (clears inherited `NO_COLOR`/`TERM=dumb`).
- **→ resources:** agent `pty.command` strings; desktop-only agents rejected.

## Invariants — do not break

1. **Detach ≠ kill**: dropping the WebSocket must leave the child running; only explicit delete kills.
2. **Sessions die with the daemon**: `RunningDaemon::stop` deletes all sessions — PTY children never orphan.
3. **Env parity with a real terminal**: spawn through the enriched env or agent CLIs will mysteriously miss PATH entries.
4. Exit is detected by polling `try_wait` and pushed as run state — attach views must always learn the child died.

## Known debt

- None tracked in the remediation plan; the module is small and stable.

---

*Source anchors: `src/core/src/pty/` (mod, runtime, manager, session), `src/server/src/web_server/ws_pty.rs`, `src/server/src/lib.rs` (shutdown deletion).*
*Last verified: v0.7.11*

<sub>[◀ Module: profiles](profiles.md) · [Documentation index](../../README.md) · [Module: previews ▶](previews.md)</sub>
