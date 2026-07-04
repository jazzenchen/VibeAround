# Web dashboard guide

The dashboard is the browser surface the daemon serves at `http://127.0.0.1:12358/` — a terminal, a chat, live previews, and runtime management in one SPA. Locally it opens pre-authenticated from the desktop app; remotely it works through a tunnel with pairing ([Tunnels and remote access](tunnels-and-remote-access.md)).

## Web Chat

A full agent conversation in the browser, sharing the same thread model as IM chats:

- **Launch selection.** Pick agent, workspace (or a cwd), and model profile per conversation; start fresh or **resume** a native CLI session from the session picker.
- **Streaming turns** with tool-call progress, a stop button, and permission cards rendered inline.
- **Slash commands** from the [IM command reference](im-usage.md) work here too (`/status`, `/new`, `/switch codex`…).
- **Modes and options.** Agents that expose session modes (e.g. permission modes) or config options show them as chat controls.
- **Idle behavior.** Web threads unload their agent after inactivity like any thread; reopening the chat replays recent output and resumes transparently.
- **Handover.** A web conversation can be picked up from IM (`/pickup`) or continued on a phone via the mobile dashboard.

## Web Terminal

A real terminal (xterm.js) attached to a PTY on your machine:

- **Sessions** are created per tool — a shell or directly into an agent CLI (`va session create --tool claude --attach` does the same from a terminal).
- Multiple **tabs**; sessions persist while the daemon runs and can be re-attached after closing the browser (`va sessions` lists them).
- **tmux integration** (optional): attach to existing tmux sessions from the dashboard; `tmux_detach_others` in settings controls whether attaching kicks other clients.
- Desktop-app-only agents (`claude-desktop`, `codex-desktop`) cannot run here — they have no CLI.

## Live Preview

Share what is running on your machine without deploying:

- **Dev server previews.** Register a local port and get a preview page that reverse-proxies it, with an iframe toolbar. Agents create these automatically when they start dev servers (via the `va-preview` skill / MCP `preview` tool).
- **Markdown preview.** Any markdown file rendered GitHub-style (`md_preview` tool or the `va-md-preview` skill).
- **Two links per preview:** the owner URL (token-authenticated, lives as long as the preview) and a share URL that expires after 10 minutes and needs no auth — safe to paste in a group chat. See [Security model](../architecture/security-model.md).
- `va previews` / `va preview delete <slug>` manage them from the CLI; preview processes started for you are killed when the daemon stops.

## Runtime management

Dashboard panels mirror what `va status` reports — channel plugin states (with restart controls), tunnel status, active agent runtimes, PTY sessions, workspaces, and model profiles. Anything you can do there also has a CLI verb ([Reference](../reference/configuration.md)).

## Mobile

The dashboard is responsive; the chat surface includes mobile command controls so thread commands are tappable. Pair once (a 60-second code confirmed from a trusted surface), then a tunneled dashboard on a phone behaves like the desktop one.

---

*Source anchors: `src/server/src/web_server/` (ws_chat, ws_pty, preview/), `src/web/src/` (SPA), `src/core/src/pty/` (sessions), `src/core/src/previews/` (owner/share, TTL), `src/skills/va-preview/`, `src/skills/va-md-preview/`.*
*Last verified: v0.7.11*

<sub>[◀ Desktop app guide](desktop-app.md) · [Documentation index](../README.md) · [IM usage ▶](im-usage.md)</sub>
