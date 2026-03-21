<div align="center">

# VibeAround

**One local-first workspace for desktop, web, and chat-based coding workflows.**

[English](README.md) | [简体中文](README_CN.md) | [Wiki](https://github.com/jazzenchen/VibeAround/wiki)

<p>
  <img src="Logo.png" width="120" alt="VibeAround" />
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Bun-1.3+-000?style=flat-square&logo=bun&logoColor=fff" alt="Bun" />
  <img src="https://img.shields.io/badge/Rust-1.78+-000?style=flat-square&logo=rust&logoColor=fff" alt="Rust" />
  <img src="https://img.shields.io/badge/Vite-6-646CFF?style=flat-square&logo=vite&logoColor=fff" alt="Vite" />
  <img src="https://img.shields.io/badge/React-19-61DAFB?style=flat-square&logo=react&logoColor=000" alt="React" />
  <img src="https://img.shields.io/badge/License-MIT-blue?style=flat-square" alt="License: MIT" />
</p>

</div>

VibeAround brings your coding agents, terminal sessions, and remote access surfaces into one runtime. Instead of treating desktop tools, browser tools, and chat entry points as separate systems, it lets them operate on the same underlying sessions, configuration, and agent lifecycle.

It is designed for people who want to:

- start work on desktop and continue from the browser
- keep long-running coding sessions reachable from more than one device
- switch between web chat, terminal, and IM channels without changing the runtime model
- manage more than one coding agent backend in one place

## Screenshots

| Desktop | Mobile |
|---------|--------|
| <img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/screenshots/pc.webp" width="720" alt="VibeAround web dashboard on desktop" /> | <img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/screenshots/mobile-claude.webp" width="200" alt="VibeAround web dashboard on mobile" /> |

## Why VibeAround

Many coding workflows break apart once you move between devices or surfaces. A terminal lives in one place, chat lives in another, and agent state becomes hard to track.

VibeAround solves that by centering the product around a shared runtime model:

- one configuration source
- one session model
- one channel-oriented routing architecture
- multiple user-facing surfaces

## What you can do today

- Open a web dashboard for terminal sessions and chat
- Launch or attach to persistent terminal sessions, including tmux-backed workflows
- Talk to supported coding agents from the web chat surface
- Access the same system through IM channels such as Telegram and Feishu
- Inspect running agents, channels, tunnels, and PTY sessions from the desktop UI
- Choose enabled agents and the default agent during onboarding

## Product surfaces

| Surface | Purpose |
|---|---|
| Desktop app | First-run setup, onboarding, runtime visibility, tray actions |
| Web dashboard | Main daily workspace for terminals, tmux sessions, and web chat |
| IM channels | Lightweight remote access through Telegram and Feishu plugins |

## Supported agents

VibeAround currently supports these coding agents:

- Claude Code
- Gemini CLI
- OpenCode
- Codex

Agent enablement and the default agent are configured in onboarding and stored in `~/.vibearound/settings.json`.

## Quick start

```bash
cd src
bun install
bun run prebuild
bun run dev
```

After startup:

1. open the desktop app
2. complete onboarding on first run
3. choose enabled agents and the default agent
4. open the web dashboard from the tray or desktop UI
5. start working through terminals, tmux sessions, or web chat

## Configuration

The active runtime configuration lives at:

- `~/.vibearound/settings.json`

Channel plugins are loaded from:

- `~/.vibearound/plugins/<channel>/dist/main.js`

## Documentation

The README stays focused on product overview and quick start. The full technical manual lives in the wiki.

Recommended reading order:

- [Wiki Home](https://github.com/jazzenchen/VibeAround/wiki)
- [Setup Guide](https://github.com/jazzenchen/VibeAround/wiki/Setup-Guide)
- [Product Surfaces](https://github.com/jazzenchen/VibeAround/wiki/Product-Surfaces)
- [Architecture](https://github.com/jazzenchen/VibeAround/wiki/Architecture)
- [Configuration Model](https://github.com/jazzenchen/VibeAround/wiki/Configuration-Model)
- [Supported Agents](https://github.com/jazzenchen/VibeAround/wiki/Supported-Agents)
- [Operational Semantics](https://github.com/jazzenchen/VibeAround/wiki/Operational-Semantics)
- [Build and Packaging](https://github.com/jazzenchen/VibeAround/wiki/Build-and-Packaging)

## Project status

VibeAround is still evolving quickly. The current architecture is already usable, but runtime behavior and product surfaces are still being refined.

The repository is public for transparency and learning. Pull requests and feature requests are not being accepted at this time.

## License

This project is licensed under the [MIT License](LICENSE).
