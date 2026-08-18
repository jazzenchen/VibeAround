# Web dashboard guide

The dashboard is the browser surface the daemon serves at `http://127.0.0.1:12358/va/` (the root path redirects there) — a terminal, a chat, live previews, and runtime management in one SPA. Locally it opens pre-authenticated from the desktop app; remotely it works through a tunnel with pairing ([Tunnels and remote access](tunnels-and-remote-access.md)).

## Web Chat

A full agent conversation in the browser, sharing the same thread model as IM chats:

- **Launch selection.** Pick agent, workspace (or a cwd), and model profile per conversation; start fresh or **resume** a native CLI session from the session picker.
- **Streaming turns** with tool-call progress, a stop button, and permission cards rendered inline.
- **Slash commands** from the [IM command reference](im-usage.md) work here too (`/status`, `/new`, `/switch codex`…).
- **Modes and options.** Agents that expose session modes (e.g. permission modes) or config options show them as chat controls.
- **Warm behavior.** Finishing a turn or closing the tab does not start an idle-shutdown timer. The host stays warm unless it later becomes the eligible least-recently-active candidate when a genuinely new host pushes the shared pool above its [soft limit](../reference/timers-and-limits.md#sizes-and-counts). Reopening the chat replays recent output; an evicted host resumes transparently on the next prompt.
- **Handover.** A web conversation can be picked up from IM (`/pickup`) or continued on a phone via the mobile dashboard.

## Web Terminal

A real terminal (xterm.js) attached to a PTY on your machine:

- **Sessions** are created per tool — a shell or directly into an agent CLI (`va session create --tool claude --attach` does the same from a terminal).
- Multiple **tabs**; sessions persist while the daemon runs and can be re-attached after closing the browser (`va sessions` lists them).
- **tmux integration** (optional): attach to existing tmux sessions from the dashboard; `tmux_detach_others` in settings controls whether attaching kicks other clients.
- Desktop-app-only agents (`claude-desktop`, `codex-desktop`) cannot run here — they have no CLI.

## Live Preview

Inspect local work without turning VibeAround into a general development tunnel:

- **One owner page.** A collapsible picker groups the current workspace and Preview list while one iframe shows the selected target. Switching targets updates the owner URL; Refresh reloads the selected content.
- **Dev server preview.** Register a local port, acknowledge the unknown-content warning, and the owner iframe loads the app. A local owner uses the loopback origin directly. After browser pairing, a tunneled owner transparently proxies normal HTTP and WebSocket/HMR traffic to that registered port on `127.0.0.1`. With a public tunnel, the Preview also gets a 10-minute code-gated Share; that narrower transport forwards authenticated GET/HEAD paths, but not writes, protocol upgrades, service workers, WebSockets, or HMR. `/va/*`, owner pages, chat, and review controls are excluded from Share. Agents create these via the `va-preview` skill / MCP `va_mcp_preview` tool ([tool reference](../reference/api-surfaces.md#mcp-tools)).
- **Markdown preview.** Pass a Markdown `file` to the same `va-preview` skill / MCP `va_mcp_preview` tool. VibeAround reads and renders it directly with the bundled `marked` script, without starting a separate static server. With a public tunnel, it also gets a copyable Share URL plus a reusable six-digit access code. Server and Markdown Share URLs, codes, and browser grants expire together after 10 minutes. See [Security model](../architecture/security-model.md).
- **Owner review.** A Preview linked to an AI task can collect text comments in Markdown and send reliable source line/section context with them. A live web app can opt into text and element comments with the development-only bridge tag returned by the `va_mcp_preview` tool; the local owner app still loads directly. Draft markers belong to the loaded page and are cleared on reload, while the task conversation remains available. Share pages never expose review controls.
- `va previews` / `va preview delete <slug>` manage them from the CLI; preview processes started for you are killed when the daemon stops.

## Runtime management

Dashboard panels mirror what `va status` reports — channel plugin states (with restart controls), tunnel status, active agent runtimes, PTY sessions, workspaces, and model profiles. Anything you can do there also has a CLI verb ([Reference](../reference/configuration.md)).

## Mobile

The dashboard is responsive; the chat surface includes mobile command controls so thread commands are tappable. Pair once (a 60-second code confirmed from a trusted surface), then a tunneled dashboard on a phone behaves like the desktop one.

---

*Source anchors: `src/server/src/web_server/` (ws_chat, ws_pty, preview/), `src/web/src/` (SPA), `src/core/src/pty/` (sessions), `src/core/src/previews/` (owner/share, TTL), `src/skills/va-preview/`.*
*Last verified: v0.7.24*

<sub>[◀ Desktop app guide](desktop-app.md) · [Documentation index](../README.md) · [IM usage ▶](im-usage.md)</sub>
