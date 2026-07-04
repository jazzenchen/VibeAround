# How it works

This page follows the two journeys that define VibeAround: an IM message reaching a coding agent, and an agent CLI being launched in your terminal. If a term is unfamiliar, see [Concepts](concepts.md).

## The runtime at a glance

```text
 web SPA / desktop UI      TUI / CLI (va)
        │ HTTP + WS              │ HTTP (va-client)
        ▼                        ▼
   ┌─────────────────────────────────────┐
   │  vibearound-server (axum daemon)    │   ◄── embedded by the desktop app
   │  HTTP · WebSocket · MCP · bridge    │
   ├─────────────────────────────────────┤
   │  core runtime                       │
   │  channels · workspace threads ·     │
   │  process supervisor · profiles ·    │
   │  PTY · previews · tunnels           │
   └───────┬──────────────────┬──────────┘
           │ stdio ACP        │ ACP
           ▼                  ▼
   channel plugin        agent CLI processes
   processes             (claude, codex, …)
   (telegram, feishu…)        │ optional loopback
                              ▼
                      /va/local-api bridge ──► upstream model APIs
```

Everything is one process plus supervised children. The desktop app embeds the same daemon the standalone `va serve` runs; every UI is a client of it.

## Journey 1: an IM message becomes an agent reply

1. **Platform to plugin.** The Telegram/Feishu/Slack plugin — a separate Node.js process supervised by the daemon — receives the platform webhook or long-poll event and normalizes it into a channel envelope: route key, message id, text, attachments.

2. **Plugin to daemon.** The envelope crosses into the daemon over the plugin's stdio ACP connection and lands in the channel input queue.

3. **Ordered dispatch.** Inputs are sharded by route key onto worker tasks: messages for the same chat are processed strictly in order, different chats in parallel. This is what keeps rapid-fire messages from racing each other.

4. **Command or prompt.** The text is checked against the slash-command grammar (`/new`, `/close`, `/switch`, `/pickup`, …). Commands are handled by the workspace-thread layer directly; everything else becomes a prompt.

5. **Route to thread.** The route resolves to its attached open thread. First contact on a route creates a thread in a default workspace and attaches the route — no setup step required.

6. **Thread to agent.** The thread ensures its host agent is alive: if needed, the supervisor spawns the agent's ACP adapter in the workspace directory, with the bound profile's environment materialized (credentials, bridge base URLs) and VibeAround's MCP endpoint and skills injected into the agent's config. An existing CLI session is resumed if the thread has one; otherwise a new session is created.

7. **Prompt and stream back.** The prompt goes to the agent over ACP. Notifications stream back — text chunks, tool-call summaries, permission requests — and are fanned out to every route attached to the thread. Permission requests render as interactive cards; the tap comes back as a callback and resolves the agent's pending request.

8. **Idle wind-down.** After the turn, a 10-minute idle timer starts. Expiry shuts the agent process down; the thread stays open, and the next message respawns the agent and resumes the same session.

## Journey 2: launching an agent CLI in your terminal

Agent Launch is a different path — instead of hosting the agent inside the daemon, VibeAround opens it in your own terminal:

1. You pick an agent, workspace, and model profile (desktop UI or `va launch --profile <name>`).
2. The profile is rendered into a concrete launch: environment variables, per-agent config overlays, and — when the profile targets a bridged provider — base URLs pointing at the daemon's local API (`http://127.0.0.1:12358/va/local-api/…`).
3. The native launcher (`va-launch`, a standalone binary shipped with both the desktop app and the CLI) validates the plan, installs project-scoped MCP/skill integrations if the daemon is running, and opens the agent in your terminal app (Terminal.app, iTerm2, PowerShell, or a Linux terminal).
4. The launched CLI talks to the bridge for models, so a Kimi or DeepSeek subscription can power Codex or Claude Code natively. Sessions created this way are discovered by the daemon and appear as resumable — which is how a terminal session can later be picked up from IM.

## Where state lives

| State | Where | Survives restart? |
|---|---|---|
| Workspaces, threads, route attachments | JSONL event logs in `~/.vibearound/` | Yes |
| Agent CLI sessions (transcripts) | Each agent's own storage | Yes (owned by the agent) |
| Model profiles, settings | `~/.vibearound/settings.json` + profile store | Yes |
| Running agent/plugin processes | In-memory, supervised | No — respawned on demand |
| Auth token for the dashboard | `~/.vibearound/auth.json` | Regenerated each daemon start |

The supervisor gives every child process (channel plugins, agent adapters) crash-respawn with heartbeat watchdogs for plugins, and cascading cleanup on daemon shutdown so no orphan processes outlive it.

---

*Source anchors: `src/server/src/lib.rs` (daemon boot, input sharding), `src/core/src/channels/` (plugin transport, dispatch), `src/core/src/workspace/` (threads, attachments), `src/core/src/process/supervisor.rs`, `src/core/src/profiles/bridge_launch.rs` (local API URLs), `src/launcher/` (va-launch).*
*Last verified: v0.7.11*
