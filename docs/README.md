# VibeAround Documentation

VibeAround lets you reach your local AI coding agents (Claude Code, Codex, Gemini CLI, and more) from the surfaces you already use: IM channels such as Telegram, Slack, and Feishu, a browser dashboard with a full terminal, a desktop control app, and a TUI/CLI. One local runtime, one workspace model, many doors into it.

**New here?** Read in this order: [What is VibeAround](product/what-is-vibearound.md) → [Install](guides/install-and-onboarding.md) → [Quick tour](guides/quick-tour.md) → [Concepts](architecture/concepts.md). Everything else is lookup material.

## Sections

| Directory | What it holds | Read it when |
|---|---|---|
| [product/](#product) | What VibeAround is and what it supports | You are evaluating it |
| [guides/](#guides) | Task-oriented how-tos | You want to get something done |
| [architecture/](#architecture) | Concepts and how the system works | You want to understand it |
| [reference/](#reference) | Lookup tables: settings, CLI, API surfaces, limits | You need to check a detail |
| [internals/](#internals) | Flow walkthroughs and per-module internals | You are debugging or changing the code |

## Product

| Page | What it answers |
|---|---|
| [What is VibeAround](product/what-is-vibearound.md) | What problem does it solve, and for whom? |
| [Supported matrix](product/supported-matrix.md) | Which agents, channels, and model providers are supported? |

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

## Architecture

| Page | What it answers |
|---|---|
| [Concepts](architecture/concepts.md) | What are workspaces, threads, routes, sessions, agents, and profiles? |
| [Overview](architecture/overview.md) | The layer diagram, every communication edge, and the module map |
| [Session lifecycle](architecture/session-lifecycle.md) | When do threads open and close? What survives a restart? |
| [Channel plugin system](architecture/channel-plugin-system.md) | How do IM integrations work under the hood? |
| [Local API and bridge](architecture/local-api-and-bridge.md) | How does the model API bridge translate between providers? |
| [Security model](architecture/security-model.md) | What is trusted, what is paired, what is tunneled? |

## Reference

| Page | Contents |
|---|---|
| [Configuration](reference/configuration.md) | settings.json, environment variables, data directory |
| [CLI](reference/cli.md) | Every `va` command |
| [API surfaces](reference/api-surfaces.md) | MCP tools, local API routes, WebSocket endpoints, preview URLs |
| [Timers and limits](reference/timers-and-limits.md) | Every timeout, TTL, interval, and size limit — the single authoritative table |
| [Provider endpoints](reference/provider-endpoints.md) | Per-provider plans, regions, base URLs, models, and credential semantics |

## Internals

For debugging and changing the code. See the [internals index](internals/README.md) for the full map. How internals content is split:

- **[architecture/](#architecture)** answers *why it is designed this way* — the model, at reader level.
- **[internals/flows/](internals/README.md#flows)** follow *one request through time* — hop by hop with code anchors.
- **[internals/modules/](internals/README.md#modules)** describe *one component in space* — responsibility, key types, invariants, known debt.

If you want to trace behavior, start from the flow; if you want to modify a component, start from the module; the flow and module pages cross-link where they meet. Cross-cutting subsystems get dedicated deep-dives — currently [Launch](internals/launch.md) (env injection, per-OS handling, producers).

## Conventions used in these docs

- `~/.vibearound/` is the data directory on every platform (override with `VIBEAROUND_DATA_DIR`).
- The local server listens on port `12358` by default.
- Shell examples use `va`, the CLI installed by `npm i @vibearound/cli`. The longer alias `vibearound` works everywhere `va` does.
- Every page ends with *Source anchors* — the code files the page derives from — and a *Last verified* version. If you change an anchored file, update the page and bump the version.
- Pages chain with prev/next links following the recommended reading order.
