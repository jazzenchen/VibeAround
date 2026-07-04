# VibeAround Documentation

VibeAround lets you reach your local AI coding agents (Claude Code, Codex, Gemini CLI, and more) from the surfaces you already use: IM channels such as Telegram, Slack, and Feishu, a browser dashboard with a full terminal, a desktop control app, and a TUI/CLI. One local runtime, one workspace model, many doors into it.

## Sections

| Directory | What it holds | Read it when |
|---|---|---|
| [product/](#product) | What VibeAround is and what it supports | You are evaluating it |
| [architecture/](#architecture) | Concepts and how the system works | You want to understand it |
| [flows/](#flows) | End-to-end walkthroughs of every major path | You are tracing behavior or debugging |
| [modules/](#modules) | Per-module internals: responsibilities, types, invariants | You are changing the code |
| [guides/](#guides) | Task-oriented how-tos | You want to get something done |
| [reference/](#reference) | Lookup tables: settings, CLI, API surfaces | You need to check a detail |

## Product

| Page | What it answers |
|---|---|
| [What is VibeAround](product/what-is-vibearound.md) | What problem does it solve, and for whom? |
| [Supported matrix](product/supported-matrix.md) | Which agents, channels, and model providers are supported? |

## Architecture

| Page | What it answers |
|---|---|
| [Concepts](architecture/concepts.md) | What are workspaces, threads, routes, sessions, agents, and profiles? |
| [Overview](architecture/overview.md) | The layer diagram, every communication edge, and the module map |
| [Session lifecycle](architecture/session-lifecycle.md) | When do threads open and close? What survives a restart? |
| [Channel plugin system](architecture/channel-plugin-system.md) | How do IM integrations work under the hood? |
| [Local API and bridge](architecture/local-api-and-bridge.md) | How does the model API bridge translate between providers? |
| [Security model](architecture/security-model.md) | What is trusted, what is paired, what is tunneled? |

## Flows

Step-by-step walkthroughs with code anchors — each page follows one path end to end.

| Page | Path |
|---|---|
| [IM message](flows/im-message.md) | Platform event → plugin → thread → agent → streamed reply |
| [Web chat](flows/web-chat.md) | WebSocket event → session intent → the same prompt path |
| [Permission](flows/permission.md) | Agent request → card in chat → tap → agent resumes |
| [Bridge request](flows/bridge-request.md) | Client dialect → translation → upstream → streamed back |
| [Agent launch](flows/agent-launch.md) | Profile → launch JSON → va-launch → terminal |
| [Handover](flows/handover.md) | Code issued → `/pickup` → route attached to the session |
| [PTY terminal](flows/pty-terminal.md) | Browser xterm ↔ WebSocket ↔ pseudo-tty |

## Modules

One page per runtime module: responsibility, key types, interactions, invariants, known debt.

| core | server |
|---|---|
| [channels](modules/channels.md) · [workspace](modules/workspace.md) · [process](modules/process.md) · [agent](modules/agent.md) · [profiles](modules/profiles.md) · [pty](modules/pty.md) · [previews](modules/previews.md) · [tunnels](modules/tunnels.md) · [auth](modules/auth.md) | [server](modules/server.md) |

## Guides

| Page | What you get done |
|---|---|
| [Install and onboarding](guides/install-and-onboarding.md) | Install the desktop app or npm CLI, finish first-run setup |
| [Quick tour](guides/quick-tour.md) | First chat, first IM channel, first handover — in 15 minutes |
| [Desktop app](guides/desktop-app.md) | Manage profiles, launches, and services from the GUI |
| [Web dashboard](guides/web-dashboard.md) | Web terminal, web chat, live preview |
| [IM usage](guides/im-usage.md) | Drive an agent from chat; full slash-command reference |
| [Connect channels](guides/connect-channels.md) | Configure Telegram, Slack, Feishu, and the others |
| [Model profiles](guides/model-profiles.md) | Provider credentials and model routing |
| [Agent launch](guides/agent-launch.md) | Launch agent CLIs in your own terminal |
| [Tunnels and remote access](guides/tunnels-and-remote-access.md) | Reach the dashboard from outside localhost |
| [Build a channel plugin](guides/build-a-channel-plugin.md) | A plugin for a new IM platform with the SDK |
| [Build from source](guides/build-from-source.md) | Compile the workspace and package the apps |
| [Troubleshooting and FAQ](guides/troubleshooting-and-faq.md) | Fix common problems |

## Reference

| Page | Contents |
|---|---|
| [Configuration](reference/configuration.md) | settings.json, environment variables, data directory |
| [CLI](reference/cli.md) | Every `va` command |
| [API surfaces](reference/api-surfaces.md) | MCP tools, local API routes, WebSocket endpoints, preview URLs |

## Conventions used in these docs

- `~/.vibearound/` is the data directory on every platform (override with `VIBEAROUND_DATA_DIR`).
- The local server listens on port `12358` by default.
- Shell examples use `va`, the CLI installed by `npm i @vibearound/cli`. The longer alias `vibearound` works everywhere `va` does.

Each page ends with *Source anchors* — the code files the page derives from — and a *Last verified* version. If you change an anchored file, update the page and bump the version.
