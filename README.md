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

**tmux-native by default** — terminal sessions can attach to tmux, so you can take unfinished work with you across devices. Start on your PC, pick it up on your phone, then resume on another machine — nothing is lost.

---

**Goals**

- Vibe Coding Everywhere!
- Small and fast from day one — Bun and Rust for a portable, always-on vibe partner.
- A context-aware programming companion in the background, without disrupting your workflow.
- **Seamless device switching:** tmux sessions persist across connections — PC → mobile → another PC → back again, zero friction.
- **Dual-track** control:
  - **Remote terminal:** attach to a live PTY from the web dashboard, with tmux session persistence.
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

**Configuration:** All runtime config (tunnel, Telegram, Feishu, working dir) is read from **`src/settings.json`**. This file is gitignored. Copy `src/settings.json.example` to `src/settings.json` and fill in the values you need. See [Configuration (settings.json)](#configuration-settingsjson) below for the full structure.

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

3. **Run the app** — starts the Tauri desktop process (tray, web server, tunnel, IM bots):

```bash
bun run dev
```

If you use **Feishu**, you need the **tunnel URL** from this step before you can set the webhook in the Feishu open platform. See [Feishu setup flow](#feishu-setup-flow-public-url-first-then-backend) below.

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

## Configuration (settings.json)

Config file path: **`src/settings.json`** (create from `src/settings.json.example`). The file is gitignored.

**Structure:**

| Path | Description |
|------|-------------|
| `tunnel.provider` | `"localtunnel"` (default), `"ngrok"`, or `"cloudflare"` |
| `tunnel.ngrok.auth_token` | Ngrok auth token (required if provider is ngrok) |
| `tunnel.ngrok.domain` | Optional reserved ngrok domain (e.g. `myapp.ngrok.io`) |
| `tunnel.preview_base_url` | Optional base URL for preview links (overrides domain when set) |
| `channels.telegram.bot_token` | Telegram bot token from [@BotFather](https://t.me/BotFather); omit to disable Telegram |
| `channels.feishu.app_id` | Feishu/Lark app ID (from open platform); omit to disable Feishu |
| `channels.feishu.app_secret` | Feishu app secret |
| `tmux.detach_others` | Detach other clients when attaching to a tmux session (default: `true`) |
| `working_dir` | Root for job workspaces (default: `~/test`) |

**Minimal example** (Telegram + Localtunnel only):

```json
{
  "tunnel": { "provider": "localtunnel" },
  "channels": {
    "telegram": { "bot_token": "YOUR_TELEGRAM_BOT_TOKEN" }
  }
}
```

**With Feishu and ngrok:**

```json
{
  "tunnel": {
    "provider": "ngrok",
    "ngrok": {
      "auth_token": "YOUR_NGROK_AUTH_TOKEN",
      "domain": "your-reserved.ngrok.io"
    }
  },
  "channels": {
    "telegram": { "bot_token": "YOUR_TELEGRAM_BOT_TOKEN" },
    "feishu": {
      "app_id": "YOUR_FEISHU_APP_ID",
      "app_secret": "YOUR_FEISHU_APP_SECRET"
    }
  }
}
```

---

### Feishu setup flow (public URL first, then backend)

Feishu sends events to your server via **webhook**. The webhook URL must be a **public HTTPS URL**. So you need a public domain (tunnel) running before you can complete Feishu configuration in the open platform.

1. **Get a public URL**
   - Run the app (`bun run dev`). It will start a tunnel (Localtunnel by default, or Ngrok if configured).
   - Note the **Tunnel URL** from the terminal (e.g. `https://xxx.loca.lt`) or from the tray → Open tunnel URL.
   - If you use Ngrok, you can set `tunnel.ngrok.domain` to a reserved domain so the URL is stable.

2. **Create a Feishu/Lark app**
   - Go to [Feishu Open Platform](https://open.feishu.cn/app) (or [Lark](https://open.larksuite.com/app) for international).
   - Create an app, then open **Credentials & Basic Info**: copy **App ID** and **App Secret** into `settings.json` under `channels.feishu.app_id` and `channels.feishu.app_secret`.

3. **Subscribe to events and set request URL**
   - In the app console, open **Event Subscriptions**.
   - Enable **Request URL (Config)**.
   - Set **Request URL** to:
     ```
     https://<YOUR-PUBLIC-DOMAIN>/api/im/feishu/event
     ```
     Example: if your tunnel URL is `https://abc123.loca.lt`, use `https://abc123.loca.lt/api/im/feishu/event`.
   - Feishu will send a `url_verification` request; the server responds with `{"challenge":"<challenge>"}`. After verification, save the configuration.
   - Under **Subscribe to events**, add **im.message.receive_v1** (receive user messages).

4. **Permissions and availability**
   - In **Permissions**, grant **Contact** (read user info) and **Send and receive messages** (e.g. `im:message:send_as_bot`, `im:message:receive_v1`) as required.
   - Under **Availability**, enable **Bot** and make the app available to your organization or to specific users.

5. **Restart and use**
   - Restart the app so it loads `settings.json` with Feishu credentials. The web server will accept POSTs at `/api/im/feishu/event`. Open a chat with your bot in Feishu and send a message to test.

**Note:** If you change the tunnel URL (e.g. new Localtunnel subdomain), update the Feishu Request URL to the new `https://<new-domain>/api/im/feishu/event`.

---

## tmux & seamless device switching

VibeAround supports attaching to tmux sessions, so you can take unfinished work with you. Start a coding session on one device, detach, and pick it up from another — the web dashboard, a different browser, or a native terminal client via SSH.

### How it works

When you create a session in the web dashboard, you can choose to attach it to a tmux session on the host machine. If you close the browser tab or lose connectivity, the tmux session keeps running in the background. Reconnect from any device and you're right back where you left off.

By default, attaching to a session detaches other clients (`tmux attach -d`). This gives you clean single-viewer semantics — open the dashboard on your phone and the previous browser tab gracefully disconnects. To allow multiple viewers on the same session, set `tmux.detach_others` to `false` in `settings.json`.

### Recommended: iTerm2 with tmux -CC integration

For the best experience when working from a Mac, use [iTerm2](https://iterm2.com)'s native tmux integration mode. Instead of rendering tmux inside a terminal emulator, iTerm2 maps each tmux window/pane to a native iTerm2 tab/split — giving you native scrollback, native copy-paste, and native keyboard shortcuts while still backed by a persistent tmux session on the remote host.

**Quick start:**

```bash
# SSH into your VibeAround host and attach with tmux -CC
ssh your-host -t "tmux -CC attach -t my-session"
```

Or from an existing SSH session:

```bash
tmux -CC attach -t my-session
```

iTerm2 will detect the `-CC` flag and switch to its native integration mode automatically.

**Why this matters for VibeAround:**

- Start a vibe coding session from the web dashboard on your PC at work.
- On the go, check progress or send quick instructions from your phone via the web dashboard or Telegram.
- At home, SSH into the same machine with `tmux -CC` from iTerm2 and get a fully native terminal experience — same session, same state, zero context loss.

The workflow is: **PC → mobile → another PC → back again**, all seamless.

---

## Roadmap

The evolution of VibeAround transitions from a basic Proof of Concept (POC) to a highly configurable, extensible orchestrator.

**Phase 1: Foundation (Current)**

- **Remote PTY terminal:** Access your local terminal from a web browser remotely, support multiple sessions, and open Claude, Gemini, or Codex directly. tmux-backed for persistent, device-portable sessions.
- **IM:** Send vibe coding tasks through Telegram and Feishu, Claude Code only for now.
- **Tunnel:** Ngrok and Localtunnel for inner-network penetration, provide a public URL for access (e.g. dashboard and Feishu webhook).

**Phase 2: Core Productization**

- **Workspaces:** Switch and manage project folders via IM or the Web Dashboard.
- **Agent settings:** Configure API keys, model choice, and generation options.
- **Skills and context:** Custom procedures, prompt templates, and project rules for the AI.
- **IM and workspaces:** Support multiple accounts and bind specific chats to specific workspaces.
- **History:** SQLite-backed conversations and task logs, resume and review anytime.

**Phase 3: Ecosystem & Extensibility**

- **More CLI tools:** Add or switch between Gemini, Codex, OpenDevin, and others via config.
- **Port discovery:** Detect new dev servers and tunnel them automatically.
- **More messaging:** Discord, Slack (Feishu already in Phase 1).
- **Plugins:** Community adapters, log sanitizers, and workflow plugins.
- **Safety and routing:** Router picks the right AI by intent, Git Sentinel snapshots before risky edits.

---

## Project status & contributing

**VibeAround is currently in an early Proof of Concept (POC) phase.**

The source code is open under the MIT License for transparency, education, and sharing the vision. **Pull Requests and feature requests are not being accepted at this time.** The architecture, core components, and APIs are changing rapidly with breaking changes; the goal is to avoid wasting your time on PRs that may conflict with the internal roadmap. The repository may open for community contributions later as the project stabilizes (Phase 2/3).

**Note on AI Generation (Dogfooding)**: A significant portion of VibeAround's codebase was generated using AI coding tools (the very "Vibe Coding" workflow we advocate). We believe this serves as a testament to the power of AI-assisted orchestration.

Feel free to fork the project, explore the code, and experiment on your own.

## License

This project is licensed under the [MIT License](LICENSE).
