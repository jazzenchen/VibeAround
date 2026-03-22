<div align="center">

# VibeAround

**Use real coding agents from your browser and chat apps.**

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

VibeAround does something simple: it brings real coding agents into the tools you already use.

It gives you access to `Claude Code`, `Gemini CLI`, `Codex`, and `OpenCode` from desktop, browser, terminals, and chat surfaces — without making the product feel like a wrapper around just one agent.

- use real coding agents, not a fake assistant
- turn chat apps into actual entry points for coding agents
- keep terminals, web chat, and IM channel access in one product
- make coding agents feel like part of your everyday workflow, not just a tool trapped in one window

## Screenshots

| Desktop | Mobile |
|---------|--------|
| <img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/screenshots/pc.webp" width="720" alt="VibeAround web dashboard on desktop" /> | <img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/screenshots/mobile-claude.webp" width="200" alt="VibeAround web dashboard on mobile" /> |

## Why VibeAround

Most AI coding products give you a single surface.

VibeAround is trying to do something much cooler: make real coding agents accessible from the tools you actually use every day.

That means you can imagine workflows like:

- driving `Claude Code` from a browser chat
- checking in on work from your phone
- using Telegram or Feishu as a real entry point to coding agents
- keeping terminal-heavy workflows available without forcing everything through the terminal UI itself

## What you can do today

- Open a web dashboard for terminals, tmux sessions, and chat
- Launch or attach to persistent PTY sessions
- Talk to supported coding agents from the web chat surface
- Reach the same agent system through IM channels such as Telegram and Feishu
- Inspect running agents, channels, tunnels, and sessions from the desktop app
- Choose enabled agents and the default agent during onboarding

## Product surfaces

| Surface | Purpose |
|---|---|
| Desktop app | Onboarding, runtime visibility, tray actions, and local control |
| Web dashboard | Main daily workspace for terminals, tmux sessions, and chat |
| IM channels | Lightweight remote access through channel plugins |

## Supported agents

VibeAround currently supports:

- Claude Code
- Gemini CLI
- OpenCode
- Codex

Agent enablement and the default agent are configured during onboarding and stored in `~/.vibearound/settings.json`.

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

Runtime configuration:

- `~/.vibearound/settings.json`

Channel plugin bundles:

- `~/.vibearound/plugins/<channel>/dist/main.js`

## Documentation

This README stays focused on product overview and fast onboarding. The wiki contains the technical and usage documentation.

Recommended starting points:

- [Wiki Home](https://github.com/jazzenchen/VibeAround/wiki)
- [Setup Guide](https://github.com/jazzenchen/VibeAround/wiki/Setup-Guide)
- [Product Surfaces](https://github.com/jazzenchen/VibeAround/wiki/Product-Surfaces)
- [Architecture](https://github.com/jazzenchen/VibeAround/wiki/Architecture)
- [Configuration Model](https://github.com/jazzenchen/VibeAround/wiki/Configuration-Model)
- [Supported Agents](https://github.com/jazzenchen/VibeAround/wiki/Supported-Agents)
- [Operational Semantics](https://github.com/jazzenchen/VibeAround/wiki/Operational-Semantics)
- [Build and Packaging](https://github.com/jazzenchen/VibeAround/wiki/Build-and-Packaging)

## Project status

VibeAround is actively evolving. The current product is already usable, while the experience and documentation continue to improve.

The repository is public for transparency and learning. Pull requests and feature requests are not being accepted at this time.

## License

This project is licensed under the [MIT License](LICENSE).
