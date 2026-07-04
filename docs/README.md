# VibeAround Documentation

VibeAround lets you reach your local AI coding agents (Claude Code, Codex, Gemini CLI, and more) from the surfaces you already use: IM channels such as Telegram, Slack, and Feishu, a browser dashboard with a full terminal, a desktop control app, and a TUI/CLI. One local runtime, one workspace model, many doors into it.

The documentation is organized in two tracks. If you want to understand what VibeAround is and how it works, start with the product track. If you want to get something done, jump into the usage track.

## Product and technical documentation

Understand the product, its concepts, and its internals.

| Page | What it answers |
|---|---|
| [What is VibeAround](what-is-vibearound.md) | What problem does it solve, and for whom? |
| [Concepts](concepts.md) | What are workspaces, threads, routes, sessions, agents, and profiles? |
| [How it works](how-it-works.md) | How does a message travel from an IM chat to a coding agent and back? |
| [Channel plugin system](channel-plugin-system.md) | How do IM integrations work under the hood? |
| [Local API and bridge](local-api-and-bridge.md) | How does the built-in model API bridge translate between providers? |
| [Security model](security-model.md) | What is trusted, what is paired, what is tunneled? |
| [Session lifecycle](session-lifecycle.md) | When do threads open and close? What survives a restart? |
| [Supported matrix](supported-matrix.md) | Which agents, channels, and model providers are supported? |

## Usage documentation

Install, configure, and operate VibeAround.

| Page | What you get done |
|---|---|
| [Install and onboarding](install-and-onboarding.md) | Install the desktop app or the npm CLI and finish first-run setup |
| [Quick tour](quick-tour.md) | Your first chat, first IM channel, and first handover in 15 minutes |
| [Desktop app guide](desktop-app-guide.md) | Manage model profiles, launches, and services from the GUI |
| [Web dashboard guide](web-dashboard-guide.md) | Use the web terminal, web chat, and live preview |
| [IM usage](im-usage.md) | Drive an agent from chat, including the full slash-command reference |
| [Connect channels](connect-channels.md) | Configure Telegram, Slack, Feishu, and the other channels |
| [Model profiles guide](model-profiles-guide.md) | Set up provider credentials and model routing |
| [Agent launch guide](agent-launch-guide.md) | Launch agent CLIs in your own terminal with saved profiles |
| [Tunnels and remote access](tunnels-and-remote-access.md) | Reach your dashboard from outside localhost |
| [Build a channel plugin](build-a-channel-plugin.md) | Write a plugin for a new IM platform with the SDK |
| [Build from source](build-from-source.md) | Compile the workspace and package the apps |
| [Reference](reference.md) | settings.json fields, CLI commands, MCP tools, data directory layout |
| [Troubleshooting and FAQ](troubleshooting-and-faq.md) | Fix common problems |

## Conventions used in these docs

- `~/.vibearound/` is the data directory on every platform (override with `VIBEAROUND_DATA_DIR`).
- The local server listens on port `12358` by default.
- Shell examples use `va`, the CLI installed by `npm i @vibearound/cli`. The longer alias `vibearound` works everywhere `va` does.

Each page ends with *Source anchors* — the code files the page derives from — and a *Last verified* version. If you change an anchored file, update the page and bump the version.
