# Flow: PTY terminal

The web terminal path: a browser xterm talking to a real pseudo-terminal on your machine. This flow is intentionally dumb in the middle — bytes in, bytes out — with the intelligence at the edges (xterm.js rendering, PTY lifecycle management).

## Hop by hop

```text
browser xterm ◄──WS /ws?session_id=──► ws_pty handler ◄──channels──► PTY runtime ◄──pty──► shell / agent CLI
```

**1. Session creation.** A terminal tab (or `va session create --tool <tool>`) creates a PTY session: a registry entry with a session id, plus a spawned child under a pseudo-terminal. The tool is either the login shell (`bash -l` on Unix, `cmd.exe` on Windows) or an agent CLI's PTY command from the registry (`claude code --permission-mode acceptEdits`, `codex`, …). Desktop-only agents are rejected here — they have no CLI.
→ `src/core/src/pty/` (registry, runtime), `src/resources/agents.json` (`pty.command`)

**2. Environment.** The child gets the enriched login-shell environment (so PATH matches your real terminal), terminal defaults (color-capable TERM; inherited `NO_COLOR`/`TERM=dumb` from GUI parents is cleared), theme hints, and any per-session extras.
→ `src/core/src/pty/runtime.rs` (`set_pty_env`), `src/core/src/process/env.rs`

**3. Attach.** The browser opens `/ws?session_id=<id>` (token-authenticated). From here it is a byte pipe: PTY output → WS to the client; WS input → PTY stdin. One JSON exception rides the text frames: `{"type":"resize","cols":…,"rows":…}` after xterm-fit recalculates, which resizes the pseudo-terminal.
→ `src/server/src/web_server/ws_pty.rs`, `mod.rs` (`ResizeMessage`)

**4. Detach vs kill.** Closing the tab drops the WebSocket but **not** the session — the child keeps running, output buffers, and a later attach resumes the view (`va session attach <id>` does the same from a real terminal). Explicit kill (`va session kill`, dashboard close, `va pty kill`) terminates the child and removes the registry entry.

**5. Exit propagation.** The runtime polls the child; when it exits, run state is pushed to the frontend so the tab can show "process exited" instead of a frozen screen.

**6. Daemon shutdown.** All PTY sessions are deleted on daemon stop — PTY children do not outlive the daemon (unlike launched terminals, which are yours).
→ `src/server/src/lib.rs` (`RunningDaemon::stop`)

## tmux attach variant

With tmux installed, the dashboard can attach to an existing tmux session instead of spawning a fresh shell: the PTY child is `tmux attach` and the `tmux.detach_others` setting decides whether other clients get kicked. Everything else in the flow is identical.
→ `src/core/src/pty/`, `src/cli` (`va tmux sessions`)

## Relationship to the other execution paths

| Path | Process owner | Protocol | Killed on daemon stop |
|---|---|---|---|
| PTY session (this flow) | daemon | raw bytes over WS | yes |
| Hosted agent ([IM flow](im-message.md)) | daemon (supervisor) | ACP over stdio | yes |
| Launched CLI ([launch flow](native-launch.md)) | your terminal | n/a (independent) | no |

A PTY session running an agent CLI is a *terminal* view of that CLI — its sessions are native sessions, discoverable and handover-able like any launched CLI's.

---

*Source anchors: `src/core/src/pty/` (registry, runtime, session), `src/server/src/web_server/ws_pty.rs` + `mod.rs` (WS handler, resize), `src/core/src/process/env.rs` (enriched env), `src/resources/agents.json` (pty commands), `src/server/src/lib.rs` (shutdown).*
*Last verified: v0.7.11*
