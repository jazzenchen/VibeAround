# VibeAround — Vibe Coding Everywhere!

<p align="center">
  <img src="Logo.png" width="120" alt="VibeAround" />
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Bun-1.3+-000?style=flat-square&logo=bun&logoColor=fff" alt="Bun" />
  <img src="https://img.shields.io/badge/Rust-1.78+-000?style=flat-square&logo=rust&logoColor=fff" alt="Rust" />
  <img src="https://img.shields.io/badge/Vite-6-646CFF?style=flat-square&logo=vite&logoColor=fff" alt="Vite" />
  <img src="https://img.shields.io/badge/React-19-61DAFB?style=flat-square&logo=react&logoColor=000" alt="React" />
  <img src="https://img.shields.io/badge/License-MIT-blue?style=flat-square" alt="License: MIT" />
</p>

---

**VibeAround** is an ambient vibe coding partner that runs on your own machine. Talk to it over the channels you already use (e.g. Telegram) and direct AI to vibe code from anywhere, at any time. It sits in the system tray as a lightweight daemon, runs a local server, and opens a web dashboard when you need it. 

---

**Goals**

- Vibe Coding Everywhere!
- Small and fast from day one — Bun and Rust for a portable, always-on vibe partner.
- A context-aware programming companion in the background, without disrupting your workflow.
- **Dual-track** control:
  - **Remote terminal:** attach to a live PTY from the web dashboard.
  - **Conversational vibe coding:** send instructions via IM; AI writes, refactors, and reviews code asynchronously.

**IM scope (current):** For the foreseeable future we only consider **one-on-one (1:1) conversations** with users. Broadcasting, group messaging, and multi-chat fan-out are explicitly out of scope and will be addressed in a later phase.

---

## Preferred setup

**TL;DR** (from repo root): `cd src` → `bun install` → `bun run prebuild` → `bun run dev`. Then tray menu → **Open Web Dashboard**; tunnel URL and password are in the terminal.

---

**Install path:** Clone the repo, then use the `src/` directory as your working path:

```
VibeAround/src/
```

**Requirements:** Bun 1.3+ and Rust 1.78+ (update Rust with `rustup update stable` if needed).

**`.env` (optional):** To enable the Telegram bot, copy `src/.env.example` to `src/.env` and set `TELEGRAM_BOT_TOKEN` (get a token from [@BotFather](https://t.me/BotFather)). If `TELEGRAM_BOT_TOKEN` is not set, the desktop app still runs but the Telegram bot is disabled. The `.env` file is gitignored and will not be committed.

**Steps (first-time or after pulling changes):**

1. **Install dependencies** — installs workspace deps for `web`, `desktop-tray`, and `desktop`:

```bash
cd src
bun install
```

2. **Build web dashboard and tray UI** — required so the local server can serve the dashboard and the desktop app can load the tray:

```bash
bun run prebuild
```

(This runs `desktop-tray:build` then `web:build` and produces `web/dist` and `desktop-tray/dist`.)

3. **Run the app** — starts the Tauri desktop process (tray, web server, Localtunnel, Telegram bot):

```bash
bun run dev
```

After the app is running:

- Use the tray menu → **Open Web Dashboard** to open the browser. The server will be at:

```
http://127.0.0.1:5182
```

- **Tunnel URL and password:** The desktop app starts Localtunnel automatically. Check the **terminal** for lines like `[VibeAround] Tunnel URL: https://xxx.loca.lt` and the tunnel password (or the link to fetch it). You can also use the tray menu → **Open tunnel URL** to open the public dashboard link.

**Note:** After the first run, you usually only need `bun run dev` unless you changed code in `web` or `desktop-tray`; then run `bun run prebuild` again before `bun run dev`. Use `bun run build` when you want to produce the full desktop app bundle (Tauri build).

---

### Run without desktop (standalone server)

If you prefer not to run the Tauri desktop app (no tray, no tunnel), you can run only the HTTP server and use the web dashboard locally:

1. From `src/`, run `bun run prebuild` so `web/dist` exists.
2. Start the server:

```bash
bun run server:dev
```

The dashboard will be at `http://127.0.0.1:5182`. The standalone server does **not** start Localtunnel or the Telegram bot; it is for local-only use (e.g. on a headless machine or when you only need the web UI).

---

## Roadmap

The evolution of VibeAround transitions from a basic Proof of Concept (POC) to a highly configurable, extensible orchestrator.

**Phase 1: Foundation (Current)**

- **CLI Engine Integration:** Seamless execution support for leading AI coding tools, currently targeting Claude Code.
- **IM Connectivity:** Core implementation of Telegram Bot integration for fluid instruction dispatching from mobile or desktop.
- **Out-of-the-box Tunneling:** Localtunnel exposes the Web Dashboard (port 5182) to a public URL so you can open the control plane from mobile; tunnel URL and password are available from the tray and console.

**Phase 2: Core Productization**

- **Dynamic Workspace Management:** Allow users to seamlessly switch, set, and manage multiple local project directories directly via IM commands or the Web Dashboard.
- **Advanced Agent Configuration:** Dedicated interfaces to manage API keys, select preferred models, and adjust generation parameters for the underlying CLI tools.
- **Skills & Context Injection:** Support for importing custom "Skills" (Standard Operating Procedures), prompt templates, and global context rules to tailor the AI's behavior to specific project guidelines.
- **Comprehensive IM Management:** Multi-account and multi-channel configurations, allowing users to bind specific Telegram chats or groups to designated local workspaces.
- **Session Persistence:** SQLite-backed history tracking to allow resuming past conversations, reviewing task logs, and recovering from unexpected interruptions.

**Phase 3: Ecosystem & Extensibility**

- **Bring Your Own CLI (BYOC):** Configuration-driven support allowing users to easily register and switch between different AI tools like Gemini CLI, Codex, or OpenDevin without modifying core code.
- **Automated Port Discovery:** Automatically detect and tunnel newly spawned local development servers without hardcoded port configurations.
- **Broader Messaging Support:** Extending control interfaces to other popular collaboration platforms such as Discord, Slack, and enterprise messaging systems.
- **Developer Plugin Ecosystem:** A secure, isolated environment allowing the community to build and share custom adapters, log sanitizers, and workflow plugins.
- **Built-in Safety & Routing Agents:**
  - **Router Agent:** Natural language intent parsing to automatically assign tasks to the most suitable underlying AI tool.
  - **Git Sentinel:** Automated workspace snapshotting and branch creation prior to executing high-risk, AI-generated code modifications.

---

## Project status & contributing

**VibeAround is currently in an early Proof of Concept (POC) phase.**

The source code is open under the MIT License for transparency, education, and sharing the vision. **Pull Requests and feature requests are not being accepted at this time.** The architecture, core components, and APIs are changing rapidly with breaking changes; the goal is to avoid wasting your time on PRs that may conflict with the internal roadmap. The repository may open for community contributions later as the project stabilizes (Phase 2/3).

**Note on AI Generation (Dogfooding)**: A significant portion of VibeAround's codebase was generated using AI coding tools (the very "Vibe Coding" workflow we advocate). We believe this serves as a testament to the power of AI-assisted orchestration.

Feel free to fork the project, explore the code, and experiment on your own.

## License

This project is licensed under the [MIT License](LICENSE).
