<div align="center">

# VibeAround — Vibe Coding Everywhere!

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
  <img src="https://img.shields.io/badge/ACP-Agent_Client_Protocol-8B5CF6?style=flat-square" alt="ACP" />
</p>

</div>

---

**VibeAround** is an ambient vibe coding partner that runs on your own machine. It provides two ways to interact with AI coding agents — a browser-based remote terminal and IM bots (Telegram, Feishu) — so you can vibe code from anywhere, at any time.

**Four AI agents, one interface** — Claude Code, Gemini CLI, OpenCode, and Codex, all connected through the [Agent Client Protocol (ACP)](https://agentclientprotocol.com/). Switch agents via IM commands.

**Browser-based remote terminal** — open a shell, attach to tmux, or quick-launch any of the four agents right from the web dashboard. Sessions persist across devices — start on your PC, check progress on your phone.

---

## Screenshots

Web dashboard on desktop and mobile — same session, any device.

| Desktop | Mobile |
|---------|--------|
| <img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/screenshots/pc.webp" width="720" alt="VibeAround web dashboard on desktop" /> | <img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/screenshots/mobile-claude.webp" width="200" alt="VibeAround web dashboard on mobile" /> |


---

## Supported Agents

VibeAround connects to AI coding agents through the [Agent Client Protocol (ACP)](https://agentclientprotocol.com/). Switch agents at any time via IM commands or the interactive card.

| Agent | Command | How it connects | Prerequisites |
|-------|---------|----------------|---------------|
| Claude Code | `/cli_claude` | In-process ACP bridge → `claude` CLI | [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code) |
| Gemini CLI | `/cli_gemini` | `gemini --experimental-acp` (native ACP) | [Gemini CLI](https://github.com/google-gemini/gemini-cli) |
| OpenCode | `/cli_opencode` | `opencode acp` (native ACP) | [OpenCode](https://github.com/opencode-ai/opencode) |
| Codex | `/cli_codex` | `npx @zed-industries/codex-acp` (ACP bridge) | Node.js 18+, [codex-acp](https://github.com/zed-industries/codex-acp) |

Use `/start` for an interactive agent picker card, or `/help` to see all commands.
> Known issue: slash commands (`/start`, `/help`, `/cli_*`) are not fully adapted in plugin channels yet.

---

**Goals**

- Vibe Coding Everywhere!
- Small and fast from day one — Bun and Rust for a portable, always-on vibe partner.
- A context-aware programming companion in the background, without disrupting your workflow.
- **Multi-agent:** Claude Code, Gemini CLI, OpenCode, Codex — all via ACP, switchable on the fly.
- **Seamless device switching:** tmux sessions persist across connections — PC → mobile → another PC → back again, zero friction.
- **Dual-track** control:
  - **Remote terminal:** attach to a live PTY from the web dashboard, with tmux session persistence.
  - **Conversational vibe coding:** send instructions via IM; AI writes, refactors, and reviews code asynchronously.

**IM scope (current):** One-on-one (1:1) conversations via the [Feishu plugin](https://github.com/jazzenchen/vibearound-plugin-feishu) and [Telegram plugin](https://github.com/jazzenchen/vibearound-plugin-telegram).

---

## Quick Start

```
cd src
bun install
bun run prebuild
bun run dev
```

Then tray menu → Open Web Dashboard; tunnel URL and password are in the terminal.

On first launch, the desktop app will show an **onboarding wizard** that walks you through agent selection, IM channel tokens, and tunnel configuration. The wizard writes everything to `~/.vibearound/settings.json`.

> **Configuration path change:** Settings are now read from `~/.vibearound/settings.json` (user home directory), not from the project `src/settings.json`. The in-repo file is only used as a development seed — on first run it is copied to `~/.vibearound/` if the file doesn't exist yet.

### Channel plugin install

VibeAround implements IM support through plugins. Official plugin source code is maintained in separate repositories:

- [vibearound-plugin-telegram](https://github.com/jazzenchen/vibearound-plugin-telegram)
- [vibearound-plugin-feishu](https://github.com/jazzenchen/vibearound-plugin-feishu)

Build plugins from their official repositories:

```
# Telegram
git clone https://github.com/jazzenchen/vibearound-plugin-telegram.git
cd vibearound-plugin-telegram
npm install
npm run build

# Feishu
git clone https://github.com/jazzenchen/vibearound-plugin-feishu.git
cd vibearound-plugin-feishu
npm install
npm run build
```

Install to runtime plugin directory:

- `~/.vibearound/plugins/telegram`
- `~/.vibearound/plugins/feishu`

Each channel plugin must provide `dist/main.js` in its plugin directory. The host loads plugins based on channel names configured in `~/.vibearound/settings.json`.

For detailed setup instructions, configuration options, and standalone server mode, see the [Setup Guide](https://github.com/jazzenchen/VibeAround/wiki/Setup-Guide) in the wiki.

---

## Known Issues

- After completing the onboarding wizard, the app currently requires a **restart** for the new configuration to take full effect (the config singleton is loaded once at startup). This will be fixed in a future release.
- Slash commands (`/start`, `/help`, `/cli_*`) are currently a known issue in plugin channels and are not fully adapted yet.

## Changelog (Recent)

### Session persistence fix

Previously, each turn (user message → agent response) created a **new** JSONL session file because the `SessionWriter` was discarded on `TurnComplete`. Now the message hub remembers the last session file path per agent and reopens it for subsequent turns, so a continuous conversation is persisted into a single session file. The `/new` command still starts a fresh session as expected.

### Gemini ACP / MCP discovery findings

Investigation revealed that Gemini CLI in `--experimental-acp` mode does **not** discover MCP servers from `.gemini/settings.json` when running under ACP. Instead, MCP servers must be injected via the `mcpServers` array in the `session/new` ACP request. This is the root cause of Gemini not having access to the `dispatch_task` tool. Fix pending — the `acp_session_loop` needs to pass MCP server config in `NewSessionRequest`.

---

## 📖 Wiki

Full configuration docs and usage guides have moved to the [Wiki](https://github.com/jazzenchen/VibeAround/wiki):

- [Setup Guide](https://github.com/jazzenchen/VibeAround/wiki/Setup-Guide) — Installation, configuration, and running the app
- [Channel Configuration](https://github.com/jazzenchen/VibeAround/wiki/Channel-Configuration) — Telegram and Feishu bot setup
- [Tunnel Configuration](https://github.com/jazzenchen/VibeAround/wiki/Tunnel-Configuration) — Localtunnel, Ngrok, and Cloudflare tunnel setup
- [tmux Guide](https://github.com/jazzenchen/VibeAround/wiki/Tmux-Guide) — tmux sessions, pane operations, and seamless device switching

---

## Roadmap

- [x] Remote PTY terminal with web dashboard, multiple sessions, tmux persistence
- [x] Tunnel support (Ngrok, Localtunnel, Cloudflare) for public URL access
- [x] Telegram bot integration
- [x] Feishu bot integration (webhook + interactive cards)
- [x] Multi-agent via ACP: Claude Code, Gemini CLI, OpenCode, Codex
- [x] Agent switching via `/cli_` commands and `/start` card
- [x] Buffered streaming output with reactions (processing / done)
- [x] Per-channel verbose config (show_thinking, show_tool_use)
- [x] Desktop onboarding wizard: first-run setup for agents, channels, and tunnel
- [ ] Next: CLI plugin system (make agent CLIs loadable as plugins)
- [ ] Workspaces: switch and manage project folders via IM or Web Dashboard
- [ ] Agent settings: model selection, API keys, generation options per agent
- [ ] Skills and context: custom procedures, prompt templates, project rules
- [ ] IM multi-account: bind specific chats to specific workspaces
- [ ] History: SQLite-backed conversations and task logs
- [ ] Port discovery: detect new dev servers and tunnel them automatically
- [ ] More messaging: Discord, Slack
- [ ] Plugins: community adapters, log sanitizers, workflow plugins
- [ ] Safety and routing: intent-based agent selection, Git Sentinel snapshots

---

## Project status & contributing

**VibeAround is currently in an early Proof of Concept (POC) phase.**

The source code is open under the MIT License for transparency, education, and sharing the vision. **Pull Requests and feature requests are not being accepted at this time.** The architecture, core components, and APIs are changing rapidly with breaking changes; the goal is to avoid wasting your time on PRs that may conflict with the internal roadmap. The repository may open for community contributions later as the project stabilizes (Phase 2/3).

**Note on AI Generation (Dogfooding)**: A significant portion of VibeAround's codebase was generated using AI coding tools (the very "Vibe Coding" workflow we advocate). We believe this serves as a testament to the power of AI-assisted orchestration.

Feel free to fork the project, explore the code, and experiment on your own.

## License

This project is licensed under the [MIT License](LICENSE).