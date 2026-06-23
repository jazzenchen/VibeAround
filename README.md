<div align="center">

<img src="Logo.svg" alt="VibeAround logo" width="96" />

# VibeAround

**An all-in-one hub for AI coding agents, keeping your vibe flow around.**

[Download](https://github.com/jazzenchen/VibeAround/releases/latest) | [Demo](https://youtu.be/6kxNKTMz-AM) | [Wiki](https://github.com/jazzenchen/VibeAround/wiki) | [Discord](https://discord.gg/KsJWkY64GN) | [WeChat](#community) | [简体中文](README_CN.md)

</div>

<p align="center">
  <img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/documents/v0.7.7/readme/launch.webp" alt="VibeAround Launch screen with agent profile and workspace selection" width="92%" />
</p>

## Why VibeAround

VibeAround keeps AI coding work together without making you rebuild the environment you already have.

- Work with the coding agents you already use, including Claude Code, Codex CLI, Gemini CLI, Pi, OpenCode, Claude Desktop, Codex Desktop, and more.
- Launch agents directly or through third-party AI APIs, without hand-editing each agent's config files back and forth.
- Bridge different AI API protocols so agents and model providers can work together even when their native APIs do not match.
- Continue the same sessions across desktop, CLI, messaging apps, mobile browsers, web browsers, and a Web Terminal.
- Preview dev servers, Markdown, and HTML remotely while execution stays on your own computer.
- Provide host-side tools like web search when the selected model provider does not offer them natively.
- Add these capabilities around your existing configs, project permissions, and workflows while keeping them as untouched as possible.

## Agent Launch

Launch the right agent with the right model.

Pick an AI agent, model profile or API endpoint, and workspace. VibeAround launches Claude Code, Codex CLI, Gemini CLI, Pi, OpenCode, Claude Desktop, Codex Desktop and more with 3rd party AI APIs, without changing each agent's own config files, skills, MCP servers, workflow, or project context.

- Launch AI coding agents and desktop apps like Claude and Codex from one desktop UI.
- Choose agent, model profile, API endpoint, workspace, terminal, and session before launch.
- Start new sessions or continue previous sessions.
- Use direct launch or profile-based launch, including profile overlays for Claude Desktop and Codex Desktop.
- Record and inspect launch-scoped API traffic, including original request, bridge request, raw response, bridge response, and search tool contents.
- Keep each agent's own config files, workflow, and project context.
- VibeAround does not modify original CLI config files. If you use tools such as CC Switch, manually remove conflicting profile fields such as `env` in `~/.claude/settings.json`.

<p align="center">
  <img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/documents/v0.7.7/readme/api-inspector.webp" alt="VibeAround Bridge recorder showing requests, responses, and search details" width="88%" />
</p>

### VibeAround vs CC Switch

| Area | VibeAround | [CC Switch](https://github.com/farion1231/cc-switch) |
|---|---|---|
| Agents | ✅ Claude Code, Codex CLI, Gemini CLI, Pi, OpenCode, Claude Desktop, Codex Desktop, Cursor CLI, Kiro CLI, Qwen Code, Trae (coming soon) | ✅ Claude Code, Claude Desktop, Codex, Gemini CLI, OpenCode, OpenClaw, and Hermes |
| Agent launch | ✅ One-click launch for both CLI and desktop agents, with selected profile, API endpoint, workspace, terminal, and session | ⚠️ Only supports launch on macOS |
| Run multiple providers at once | ✅ Run the same agent with different API profiles across sessions | ❌ Must switch profiles / active configs |
| API bridge | ✅ OpenAI Responses, Chat Completions, Anthropic Messages, and Gemini Generate Content | ✅ OpenAI Responses, Chat Completions, Anthropic Messages, and Gemini Generate Content |
| Live request inspect | ✅ Original request, bridge request, raw response, bridge response, and search tool contents | ❌ Not currently supported |
| Session resume | ✅ Resume and launch both CLI and desktop agents on macOS, Windows, and Linux | ⚠️ macOS terminal resume; Windows and Linux copy the command to clipboard |
| Workspace selection | ✅ Launch agents from a specified directory | ⚠️ Only supports OpenClaw workspace |
| IM Chat | ✅ [Remote Messaging & Session Continuity](#remote-messaging--session-continuity) through Feishu/Lark, Discord, Slack, and more | ❌ Not currently supported |
| Web Terminal | ✅ [Web Terminal](#web-terminal) for remote CLI control | ❌ Not currently supported |
| Web Hub | ✅ [Web Hub](#web-hub) for browser-based launch, sessions, and chat | ❌ Not currently supported |
| Remote preview | ✅ [Live Preview](#live-preview) for dev server / Markdown / HTML links | ❌ Not currently supported |
| Host-side web search | ✅ [Host-side Web Search](#host-side-web-search) via `va-search-tool` when providers do not expose native search | ❌ Not currently supported |
| MCP and Skills | ❌ Not currently supported | ✅ Unified MCP and Skills management across supported apps |
| Usage and cost tracking | 🚧 Roadmap | ✅ Built-in usage dashboard |

## Known Issue

### Why do I see `Unable to connect to API (ConnectionRefused)`?

This usually means the agent is still reading API settings written by CC Switch or another config-switching tool. Those settings can point the agent to a local proxy that is no longer running, so the agent refuses the connection before VibeAround can route the request.

Remove the conflicting fields from the corresponding agent config file, then launch the agent again from VibeAround. For Claude Code, check `~/.claude/settings.json` and remove stale provider fields such as an `env` block that sets a base URL, API key, or proxy endpoint. If you also have project-level Claude config files, check those as well.

## API Profiles & Bridge

Use more model providers with the AI agents you prefer. Save provider keys, base URLs, models, and aliases as profiles; when an agent and a provider speak different API shapes, VibeAround's local bridge translates between them.

```text
Agent-facing API shapes             VibeAround API Bridge               Provider API shapes
+-------------------------+         +-----------------------------+      +-------------------------+
| OpenAI Responses        | ----\   | profile-scoped local routes |  /-> | OpenAI Responses        |
| OpenAI Chat Completions | -----\  | model aliases and metadata  | /--> | OpenAI Chat Completions |
| Anthropic Messages      | ------> | request/response translate  | ---> | Anthropic Messages      |
| Gemini Generate Content | -----/  | va-ai-api-bridge (VAAAB)    | \--> | Gemini Generate Content |
+-------------------------+         +-----------------------------+      +-------------------------+
```

The bridge is powered by [va-ai-api-bridge](https://github.com/jazzenchen/va-ai-api-bridge), the standalone VAAAB project behind VibeAround's API translation.

<p align="center">
  <img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/documents/v0.7.7/readme/api-bridge.webp" alt="VibeAround API Bridge connection settings" width="88%" />
</p>

| API shape | Common endpoint |
|---|---|
| OpenAI Responses | `/v1/responses` |
| OpenAI Chat Completions | `/v1/chat/completions` |
| Anthropic Messages | `/v1/messages` |
| Gemini Generate Content | `/v1beta/models/{model}:generateContent` |

Built-in provider presets include DeepSeek, Alibaba DashScope, Moonshot / Kimi, MiniMax, Xiaomi MiMo, xAI / Grok, NVIDIA NIM, Z.AI / GLM, Google Gemini, OpenRouter, Azure OpenAI, and custom endpoints.

- Save API settings with keys, base URLs, models, aliases, and metadata.
- Bridge OpenAI Responses, OpenAI Chat Completions, Anthropic Messages, and Gemini Generate Content shapes.
- Run the same AI agent with different API profiles across sessions.

## Host-side Web Search

Give agents web search even when the selected model provider does not expose native server-side search.

VibeAround can replace provider-native `web_search` with a local search runtime, then feed normalized results back into the model through the bridge. Search sources are configured in Settings and run through the [va-search-tool](https://github.com/jazzenchen/va-search-tool) project. The same search SDK/runtime can also run standalone from the command line for local smoke tests or custom integrations.

### va-search-tool

[va-search-tool](https://github.com/jazzenchen/va-search-tool) is the standalone search runtime behind VibeAround host-side web search. It can run as a supervised VibeAround plugin over stdio, as a CLI for one-off searches, or as a small local HTTP service with `/v1/search`. It currently supports Exa, Tavily, Grok / xAI, and Brave Search.

- Use Exa, Tavily, Grok / xAI, and Brave Search as host-side sources.
- Run standalone CLI checks with [`va-search-tool`](https://github.com/jazzenchen/va-search-tool) `search ...` or expose a local `/v1/search` service when you want the search runtime outside VibeAround.
- Test search settings before saving from the Web Search settings page.
- Inspect search requests and source-separated results in the Bridge recorder.
- Keep API keys in local settings while the AI provider only receives normalized search results.

## Agent as API

Use local AI coding agents as API endpoints for development and local testing.

VibeAround can expose enabled local agents as OpenAI / Anthropic-compatible APIs, so you can test app integrations against Claude Code, Codex CLI, Gemini CLI, OpenCode, and other local agents without deploying a production gateway.

<p align="center">
  <img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/documents/v0.7.7/readme/agent-as-api.webp" alt="VibeAround Local API Workbench for testing coding agents as APIs" width="88%" />
</p>

| API shape | Local agent endpoint |
|---|---|
| OpenAI Responses | `/local-agent/{agent_id}/{profile_id}/v1/responses` |
| OpenAI Chat Completions | `/local-agent/{agent_id}/{profile_id}/v1/chat/completions` |
| Anthropic Messages | `/local-agent/{agent_id}/{profile_id}/v1/messages` |
| Models | `/local-agent/{agent_id}/{profile_id}/v1/models` |

- Test local agents from the Local API Workbench before wiring them into your app.
- Select the agent and profile through VibeAround, then call the matching local endpoint.
- Pass `x-vibearound-cwd` to run the request in a specific workspace directory.
- Use streaming or non-streaming requests with the supported API shapes.
- Keep this for dev and local test usage; VibeAround is not trying to be a hosted production API gateway.

## Remote Messaging & Session Continuity

Hand over a running coding session, then pick it up from anywhere.

VibeAround can prepare a `/pickup` command for the current desktop or CLI session. Paste that command into any connected IM channel to continue the same session with the same agent; the pickup is not tied to the channel where it was created.

IM integrations are built through the [VibeAround Channel SDK](https://github.com/jazzenchen/va-plugin-channel-sdk) and plugin system, so each messaging platform can run as a managed plugin around the same session and command model instead of being hard-coded into the core app.

### VibeAround Channel SDK

[VibeAround Channel SDK](https://github.com/jazzenchen/va-plugin-channel-sdk) is the SDK for building IM channel plugins. It handles the VibeAround agent/session lifecycle and command bridge, while each plugin focuses on the platform-specific transport and rendering, such as Feishu/Lark, Telegram, Slack, Discord, DingTalk, WeCom, or QQ Bot.

<p align="center">
  <img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/documents/v0.7.7/readme/im-remote.webp" alt="VibeAround Remote Access settings for messaging apps and Cloudflare tunnel" width="88%" />
</p>

- Hand over the current session from desktop, CLI, or Web Hub.
- Pick it up from Feishu/Lark, Discord, Slack, or another configured messaging channel.
- Add or update messaging channels through SDK-based plugins.
- Continue the same agent session remotely without losing context.
- Chat directly with AI agents from messaging apps after pickup.
- Switch workspaces and agents with `/switch`.
- Open preview links from the same conversation.

### VibeAround vs cc-connect

| Area | VibeAround | [cc-connect](https://github.com/chenhg5/cc-connect) |
|---|---|---|
| Handover / pickup | ✅ Generate a `/pickup` command from the current session and resume it in any connected IM channel | ❌ Not currently supported |
| Chat platforms | ✅ Feishu/Lark, Discord, Slack, Telegram, WeChat, DingTalk, WeCom, QQ Bot | ✅ Feishu/Lark, WPS Xiezuo, DingTalk, Telegram, Slack, Discord, LINE, WeCom, Weibo, Weixin, QQ, QQ Bot, Matrix |
| Agent connection | ✅ Works with VibeAround-launched agents and their selected profiles/workspaces | ✅ Works with 10+ agents and ACP-compatible agents |
| Session commands | ✅ `/session --list`, `/session --switch`, `/new`, and `/pickup` | ✅ `/new`, `/list`, `/switch`, and `/current` |
| Agent / profile switch | ✅ `/switch`, `/agent --switch`, and `/profile --switch` | ❌ Not currently supported |
| Workspace commands | ✅ `/workspace --list` and `/workspace --switch` | ✅ `/dir` and `/cd` |
| Remote preview | ✅ Sends Live Preview links for dev servers, Markdown, and HTML | ❌ Not currently supported |
| IM file attachments | ⚠️ Send-only; receiving files from IM is not currently supported | ✅ Send and receive files/images on supported platforms |
| Web Terminal | ✅ Browser terminal for controlling local AI agent CLIs remotely | ❌ Not currently supported |
| Web Hub | ✅ Browser launch, session continuation, and chat | ⚠️ Web admin/config dashboard; service runs separately |
| Scheduling and rich IM commands | ❌ Not currently supported | ✅ `/timer`, `/cron`, `/cancel`, `/ps`, provider/model/mode commands |

## Web Terminal

Control local AI agents from desktop or mobile browsers.

VibeAround exposes local terminal sessions through the web dashboard so you can drive AI agent CLIs from desktop or mobile browsers while execution stays on your computer.

<p align="center">
  <img src="assets/marketing/screenshots/en/web-terminal.png" alt="VibeAround Web Terminal for remote agent control" width="88%" />
</p>

- Access local AI agent CLIs through Web Terminal from desktop or mobile browsers.
- Use tunnels for remote access while the daemon stays local.
- Keep local project permissions and terminal environment in control.

## Web Hub

Launch and continue sessions from desktop or mobile browsers.

VibeAround Web Hub gives you a browser interface for choosing agents, API profiles, workspaces, and sessions, then continuing the same work across devices.

<table>
  <tr>
    <td width="72%" align="center"><img src="assets/marketing/screenshots/en/cover-web-dashboard.png" alt="VibeAround web dashboard cover" width="92%" /></td>
    <td width="28%" align="center"><img src="assets/marketing/screenshots/en/web-dashboard-mobile.png" alt="VibeAround mobile web dashboard" width="220" /></td>
  </tr>
</table>

- Start new browser sessions or continue existing ones.
- Pick agents, API profiles, workspaces, and sessions from the browser.
- Use desktop and mobile browsers without moving execution to a cloud server.

## Remote Tunnels

Expose VibeAround's local web surfaces only when you choose to.

Remote tunnels are used by Web Hub, Web Terminal, Live Preview, and Markdown preview links. VibeAround keeps the daemon local, starts the selected tunnel provider, and requires browser pairing for public tunnel URLs.

| Tunnel option | Status | Notes |
|---|---|---|
| Local only | ✅ Supported | No public tunnel; the daemon listens on loopback. |
| Localtunnel | ✅ Supported | Quick public URL through the managed `localtunnel` npm package or system `npx`. |
| Cloudflare Tunnel | ✅ Supported | Named tunnel via `cloudflared`, tunnel token, and configured hostname. |
| ngrok | ✅ Supported | Uses the ngrok SDK with auth token and optional reserved/static domain. |
| Tailscale Funnel | 🚧 Roadmap | Planned for users who already keep machines connected through Tailscale. |

## Live Preview

Preview what AI agents are building.

VibeAround turns dev servers, Markdown files, and HTML files into previewable links you can open from desktop browsers, mobile browsers, or messaging apps.

<p align="center">
  <img src="assets/marketing/screenshots/en/preview-in-a-row.png" alt="Ask for previews from messaging apps, pair a browser, and open web or Markdown previews" width="92%" />
</p>

- Generate owner links and scoped short-lived share links.
- Use tunnels to access preview links remotely.
- Preview dev servers, Markdown files, and HTML files.

## Supported AI Agents

| Agent | Launch | Continue / handover | Profile routing |
|---|---:|---:|---:|
| Claude Code | ✅ | ✅ | ✅ |
| Claude Desktop | ✅ | — | ✅ |
| Codex CLI | ✅ | ✅ | ✅ |
| Codex Desktop | ✅ | — | ✅ |
| Pi | ✅ | ✅ | ✅ |
| Gemini CLI | ✅ | ✅ | ✅ |
| OpenCode | ✅ | ⚠️ | ✅ |
| Cursor CLI | ➜ | ✅ | — |
| Kiro CLI | ➜ | ✅ | — |
| Qwen Code | ➜ | ✅ | — |

✅ Supported · ⚠️ Partial · ➜ Direct launch · — Not supported

## Supported Providers

Preset profiles cover common official and compatible providers. Custom endpoints are available when your provider speaks a supported API shape.

| Provider | Notes |
|---|---|
| DeepSeek | OpenAI-compatible and bridge routes, model aliases, Claude suffix normalization |
| Alibaba DashScope | Coding Plan and Token Plan endpoints |
| Moonshot / Kimi | OpenAI-compatible and Anthropic-style bridge flows |
| MiniMax | OpenAI-compatible and Anthropic-style bridge flows |
| Xiaomi MiMo | Token Plan and regional endpoints with provider quirks handled |
| xAI / Grok | Responses and Chat variants |
| NVIDIA NIM | OpenAI-compatible Chat Completions |
| Z.AI / GLM | Built-in compatible endpoint |
| Google Gemini | Native Gemini API profile |
| OpenRouter | OpenAI-compatible profile |
| Azure OpenAI | Responses and Chat deployment routing |
| Custom endpoint | Bring your own base URL, headers, models, and API kinds |

## Messaging Channels

Channel plugins are built with the [VibeAround Channel SDK](https://github.com/jazzenchen/va-plugin-channel-sdk), then installed and managed by VibeAround.

| Channel | Setup style | Typical use |
|---|---|---|
| Telegram | BotFather token | Personal bot and mobile chat |
| Feishu / Lark | App credentials | Team IM and enterprise bot |
| Discord | Bot token | Server and DM workflows |
| Slack | Bot/App token with Socket Mode | Workspace DM workflows |
| WeChat | QR login through OpenClaw-compatible bridge | Chinese personal chat |
| DingTalk | Stream API credentials | Enterprise chat |
| WeCom | WebSocket bot credentials | Enterprise WeChat workflows |
| QQ Bot | Guild bot credentials | QQ bot workflows |

## Local-first Security

VibeAround keeps AI coding work on your computer by default.

- Agents run on your computer.
- Provider credentials stay in local VibeAround settings/profile storage.
- The daemon listens on loopback unless you explicitly enable a tunnel.
- Dashboard APIs and WebSocket routes require a local auth token.
- Public tunnel URLs require browser pairing.
- Preview links are scoped and short-lived.
- Agent CLIs use your local project permissions.

## Quick Start

1. Download the latest desktop package for your platform.
2. Open VibeAround and complete onboarding.
3. Enable the agent CLIs you use.
4. Add API profiles if you want VibeAround to route model traffic.
5. Pick an agent, model profile, terminal, workspace, and session from Launch.
6. Continue from desktop, Web Hub, Web Terminal, or a configured messaging channel.

Detailed guides live in the [Wiki](https://github.com/jazzenchen/VibeAround/wiki).

## Download

Latest release: [VibeAround v0.7.9](https://github.com/jazzenchen/VibeAround/releases/tag/v0.7.9).

| Platform | Recommended download |
|---|---|
| macOS Apple Silicon | [VibeAround-macOS-arm64-0.7.9.dmg](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.9/VibeAround-macOS-arm64-0.7.9.dmg) |
| Windows x64 | [Setup EXE](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.9/VibeAround-Windows-x64-Setup-0.7.9.exe), [MSI](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.9/VibeAround-Windows-x64-MSI-0.7.9.msi), or [portable ZIP](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.9/VibeAround-Windows-x64-Portable-0.7.9.zip) |
| Linux x64 | [AppImage](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.9/VibeAround-Linux-x64-AppImage-0.7.9.AppImage) or [deb](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.9/VibeAround-Linux-x64-DEB-0.7.9.deb) |

Windows and Linux packages are built by GitHub Actions. The macOS package is currently Apple Silicon only.

<a id="migration-guide-from-06x"></a>

### Migration Guide From 0.6.x

v0.7.3 changes Startkit state, detected agent sources, desktop launch targets, and profile launch settings. If you are upgrading from 0.6.x, do a clean local-state migration:

1. Quit VibeAround.
2. Make a full backup of the old `~/.vibearound` directory.
3. Remove the old `~/.vibearound` directory.
4. Restore only durable state from the backup.
5. Launch VibeAround v0.7.3 and rerun onboarding / Startkit setup if Launch, profile, Startkit, or desktop-agent settings look stale.

Restore these durable items only: `settings.json`, `profiles/`, `google-oauth/`, `agents.json`, `launcher.json`, `state/`, `sessions/`, `launch-session-archive.json`, `workspaces/`, and `worktrees/`.

Do not restore generated or runtime data such as `.cache/`, `cache/startkit/`, `agents.detected.json`, `desktop-apps.detected.json`, `profile-state/`, `api-bridge/launches/`, `logs/`, `npm-global/`, `plugins/`, `bin/`, `runtime/`, or `auth.json`.

macOS / Linux:

```bash
set -euo pipefail

BACKUP="$HOME/vibearound-0.6-full-backup-$(date +%Y%m%d%H%M%S)"
SOURCE="$HOME/.vibearound"

if [ -d "$SOURCE" ]; then
  cp -a "$SOURCE" "$BACKUP"
  rm -rf "$SOURCE"
fi

mkdir -p "$SOURCE"

for item in settings.json profiles google-oauth agents.json launcher.json state sessions launch-session-archive.json workspaces worktrees; do
  [ -e "$BACKUP/$item" ] && cp -a "$BACKUP/$item" "$SOURCE/"
done
```

Windows PowerShell:

```powershell
$ErrorActionPreference = "Stop"

$Backup = Join-Path $env:USERPROFILE ("vibearound-0.6-full-backup-" + (Get-Date -Format "yyyyMMddHHmmss"))
$SourceRoot = Join-Path $env:USERPROFILE ".vibearound"

if (Test-Path $SourceRoot) {
  Copy-Item $SourceRoot $Backup -Recurse -Force
  Remove-Item $SourceRoot -Recurse -Force
}

New-Item -ItemType Directory -Force -Path $SourceRoot | Out-Null

$Items = @(
  "settings.json", "profiles", "google-oauth", "agents.json", "launcher.json",
  "state", "sessions", "launch-session-archive.json", "workspaces", "worktrees"
)

foreach ($Item in $Items) {
  $Source = Join-Path $Backup $Item
  if (Test-Path $Source) { Copy-Item $Source $SourceRoot -Recurse -Force }
}
```

## Develop Locally

```bash
cd src
bun install
bun run prebuild
bun run dev
```

Prerequisites: Rust 1.82+, Bun 1.3+, and Node.js 24 LTS recommended. macOS also needs Xcode command line tools; Linux needs the WebKitGTK/Tauri system dependencies for your distribution.

## Documentation

Documentation is still under construction and may lag behind fast-moving features.

- [Setup Guide](https://github.com/jazzenchen/VibeAround/wiki/Setup-Guide)
- [Launch, Profiles, And Models](https://github.com/jazzenchen/VibeAround/wiki/Model-Profiles-and-Agent-Launch)
- [Supported Agents](https://github.com/jazzenchen/VibeAround/wiki/Supported-Agents)
- [Channels](https://github.com/jazzenchen/VibeAround/wiki/Channel-Plugins)
- [Configuration Model](https://github.com/jazzenchen/VibeAround/wiki/Configuration-Model)
- [Tunnels And Previews](https://github.com/jazzenchen/VibeAround/wiki/Tunnel-Configuration)
- [Architecture](https://github.com/jazzenchen/VibeAround/wiki/Architecture)
- [Build And Packaging](https://github.com/jazzenchen/VibeAround/wiki/Build-and-Packaging)
- [FAQ And Troubleshooting](https://github.com/jazzenchen/VibeAround/wiki/FAQ-and-Troubleshooting)

## Community

VibeAround is still early, moving fast, and mostly one-person work. My goal is to build an all-in-one agent hub that helps more people embrace AI with less friction and more clarity. Some edges are still rough, from test coverage to interaction details, and I sincerely hope VibeAround can be shaped together with the community. Discussions, code contributions, ideas, shared workflows, and bug reports are always welcome.

[![Discord](https://img.shields.io/badge/Discord-Join%20VibeAround-5865F2?logo=discord&logoColor=white)](https://discord.gg/KsJWkY64GN)
[![Product Hunt](https://img.shields.io/badge/Product%20Hunt-Follow%20VibeAround-DA552F?logo=producthunt&logoColor=white)](https://www.producthunt.com/products/vibearound)

Friendly community: [LINUX DO](https://linux.do)

WeChat group for Chinese-language discussion:

<img src="assets/community/wechat-group-qr-2026-06-28.webp" width="180" alt="VibeAround WeChat group QR code, valid until June 28, 2026" />

This WeChat QR code is valid until June 28, 2026. Use Discord or GitHub Issues to ask for the latest one if it has expired.

## License

[MIT](LICENSE)
