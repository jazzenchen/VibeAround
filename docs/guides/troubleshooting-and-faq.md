# Troubleshooting and FAQ

Symptoms first, concepts second. When in doubt, start with `va doctor` — it checks endpoint, auth, and server health in one shot.

## Connectivity and auth

**The dashboard shows 401 / asks for auth after a restart.**
Expected: the auth token regenerates on every daemon start. Reopen from a trusted entry (desktop tray → Dashboard, or the URL from `va status`); remote browsers must re-pair.

**An agent CLI I launched shows `Unable to connect to API (ConnectionRefused)`.**
The CLI was launched with a bridged profile, and the daemon is not running — the bridge at `127.0.0.1:12358` is its model endpoint. Start the desktop app or `va serve`, then retry the request in the CLI.

**Port 12358 is already in use.**
Another VibeAround instance (or a stale process from a crash) holds it. `va status` tells you if a healthy daemon is answering; otherwise find and kill the holder. On macOS and Linux child processes die with their daemon; on Windows the daemon sweeps orphaned child processes at startup. A previous *daemon* process must be stopped by you.

**Pairing code always invalid or expired.**
Codes live 60 seconds. Generate, then confirm immediately from a chat connected to the same daemon (`/pair <code>`) or locally. A code from machine A never works on machine B's daemon.

## Channels

**The plugin shows crashed / keeps restarting.**
Almost always credentials or platform config. Check daemon logs (the plugin's stderr is tagged with its channel kind), fix `channels.<kind>`, then `va channel restart <kind>`.

**The bot doesn't respond at all.**
`va channels` — is the plugin running? Is the channel configured (a plugin without `channels.<kind>` config stays disabled)? Platform side: webhook URL reachable / long-poll started? Then check whether `/status` in the chat responds — if commands work but prompts don't, look at agent errors instead.

**Buttons on permission cards do nothing.**
Update the plugin — action callbacks require a current plugin + SDK. On Feishu specifically, cards must use the V2 card schema; V1 action tags are not supported by the platform.

**Messages arrive out of order / interleave between chats.**
Within one chat, processing is strictly ordered. Across chats it is parallel by design. If a single chat sees interleaving, you likely have two bots (two routes) in one group — each bot keeps its own thread.

## Agents and threads

**"Workspace thread is closed" errors.**
The thread was closed (by `/close`, or auto-closed after an unrecoverable agent error). Send `/new` and continue.

**The agent stopped replying mid-turn.**
Stop the turn (stop button / plugin stop), check `/status`. If the agent process crashed, the next prompt spawns a fresh one and resumes the session. Persistent failures right at spawn usually mean the agent CLI is missing or needs login — try launching it manually once.

**`/switch` to another agent lost my context.**
Expected: switching to a different agent creates a new thread with a fresh session — context does not carry between agent products. The old conversation is still there: `/session` lists it, `/session --switch <id>` re-attaches it. Switching only the profile (same agent) keeps the session. See [Session lifecycle](../architecture/session-lifecycle.md).

**Authentication required errors from an agent.**
The agent CLI itself needs a vendor login (`claude login`, etc.) — VibeAround hosts it but cannot log in for you. The thread auto-closes on this error; log in in a terminal, then `/new`.

**Does my conversation survive a daemon restart?**
Yes, in the sense that matters: threads, route attachments, and CLI session ids are persisted, and the agent's transcript lives in the agent's own storage. A turn that was mid-flight is lost. The old "sessions live only in memory" limitation no longer applies.

## Sessions, handover, previews

**`/pickup` says the code is invalid.**
Codes are one-shot and short-lived — issue a fresh one and use it immediately. Both surfaces must talk to the same daemon.

**`/session --switch` can't find my terminal session.**
`va launch sessions` shows what discovery sees. The session must belong to the same agent and workspace the chat is bound to; archived sessions are hidden (unarchive with `va launch unarchive`).

**A Preview Share link stopped working after a few minutes.**
Each Server or Markdown Share URL, six-digit access code, and browser grant expires after the same 600-second deadline by design. Re-share to create a new transaction, or use the owner link where you are authenticated. The Server Share proxy forwards authenticated GET/HEAD paths unchanged, including page data reads. Writes, protocol upgrades, service workers, WebSockets, and HMR are unsupported; `/va/*`, owner pages, chat, and review controls are excluded. It is not a general API-compatibility or API-isolation layer. See [Security model](../architecture/security-model.md).

## Models and profiles

**The agent rejects the model name.**
Use the profile's alias model id (agents often validate model names against their vendor's list). The [Model profiles guide](model-profiles.md#troubleshooting-profiles) has a fuller table for 401s, capability mismatches, and rate limits.

## Platform quirks

- **macOS Intel:** source-build only for the desktop app ([Build from source](build-from-source.md)).
- **Linux terminal launch** depends on your desktop environment; VibeAround tries `xdg-terminal-exec`, `x-terminal-emulator`, and common terminals (GNOME Terminal, Konsole, XFCE Terminal, xterm, Kitty, Alacritty, WezTerm).
- **Windows restart flakiness** ("port busy" right after restart) is retried automatically; if it persists, an old process is genuinely alive.

## Where are the logs?

Daemon logs are written to `~/.vibearound/logs/runtime/vibearound.log.<date>` (daily rolling files, ANSI-free) and mirrored to stdout / the desktop app's log view. Plugin stderr is embedded in the daemon log, tagged by channel kind.

The filter is the standard `RUST_LOG` env var; the default is `info,common=debug` (verbose runtime, quiet dependencies). For a debug session:

```bash
RUST_LOG=debug va serve                      # everything at debug
RUST_LOG=warn,common::channels=trace va serve  # trace one subsystem
```

When reporting an issue, `va doctor` output plus the relevant slice of the current log file is the useful minimum.

---

*Source anchors: `src/core/src/auth/pair.rs` (code TTL), `src/core/src/channels/prompt/mod.rs` (auto-close reasons), `src/core/src/workspace/` (persistence), `src/core/src/previews/store.rs` (share TTL), `src/server/src/lib.rs` (orphan sweep on Windows, Windows bind retry), `src/core/src/logging.rs` (log destinations).*
*Last verified: v0.7.11*

<sub>[◀ Build from source](build-from-source.md) · [Documentation index](../README.md) · [Concepts ▶](../architecture/concepts.md)</sub>
