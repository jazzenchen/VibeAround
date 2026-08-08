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

- **Dev server previews are local-only.** Register a local port and get a same-machine preview page that reverse-proxies page and static-resource GET/HEAD requests, with an iframe toolbar. Agents create these via the `va-preview` skill / MCP `preview` tool ([tool reference](../reference/api-surfaces.md#mcp-tools)). Fetch/XHR, writes, WebSockets, HMR, and public-host access are outside this preview boundary.
- **Markdown preview.** Any Markdown file gets an owner URL. With a public tunnel, it also gets a copyable Share URL plus a reusable six-digit access code; both expire together after 10 minutes (`md_preview` tool or the `va-md-preview` skill). See [Security model](../architecture/security-model.md).
- `va previews` / `va preview delete <slug>` manage them from the CLI; preview processes started for you are killed when the daemon stops.

## Runtime management

Dashboard panels mirror what `va status` reports — channel plugin states (with restart controls), tunnel status, active agent runtimes, PTY sessions, workspaces, and model profiles. Anything you can do there also has a CLI verb ([Reference](../reference/configuration.md)).

## Mobile

The dashboard is responsive; the chat surface includes mobile command controls so thread commands are tappable. Pair once (a 60-second code confirmed from a trusted surface), then a tunneled dashboard on a phone behaves like the desktop one.

---

*Source anchors: `src/server/src/web_server/` (ws_chat, ws_pty, preview/), `src/web/src/` (SPA), `src/core/src/pty/` (sessions), `src/core/src/previews/` (owner/share, TTL), `src/skills/va-preview/`, `src/skills/va-md-preview/`.*
*Last verified: v0.7.11*

<sub>[◀ Desktop app guide](desktop-app.md) · [Documentation index](../README.md) · [IM usage ▶](im-usage.md)</sub>
