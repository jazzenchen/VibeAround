<div align="center">

# VibeAround — 随时随地 Vibe Coding！

[English](README.md) | [简体中文](README_CN.md)

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

**VibeAround** 是一个运行在你自己机器上的 AI 编程伙伴。通过你日常使用的 IM（Telegram、飞书）与它对话，随时随地指挥 AI 写代码。它以轻量守护进程的形式驻留在系统托盘，运行本地服务器，需要时打开 Web 仪表盘。

**四个 AI Agent，一个入口** — 在 Claude Code、Gemini CLI、OpenCode、Codex 之间一键切换。所有 Agent 通过 [Agent Client Protocol (ACP)](https://agentclientprotocol.com/) 统一通信，无论哪个 AI 在干活，体验都是一致的。

**原生 tmux 支持** — 终端会话可以挂载到 tmux，跨设备无缝衔接。在 PC 上开始，手机上查看进度，换台电脑继续 — 什么都不会丢。

---

## 截图

Web 仪表盘在桌面和移动端 — 同一个会话，任何设备。

| 桌面端 | 移动端 |
|--------|--------|
| <img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/screenshots/terminal-pc.webp" width="600" alt="桌面端 Web 仪表盘" /> | <img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/screenshots/terminal-mobile.webp" width="200" alt="移动端 Web 仪表盘" /> |

---

## 支持的 Agent

VibeAround 通过 [Agent Client Protocol (ACP)](https://agentclientprotocol.com/) 连接 AI 编程 Agent。随时通过 IM 命令或交互卡片切换。

| Agent | 命令 | 连接方式 | 前置依赖 |
|-------|------|---------|---------|
| Claude Code | `/cli_claude` | 进程内 ACP 桥接 → `claude` CLI | [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code) |
| Gemini CLI | `/cli_gemini` | `gemini --experimental-acp`（原生 ACP） | [Gemini CLI](https://github.com/google-gemini/gemini-cli) |
| OpenCode | `/cli_opencode` | `opencode acp`（原生 ACP） | [OpenCode](https://github.com/opencode-ai/opencode) |
| Codex | `/cli_codex` | `npx @zed-industries/codex-acp`（ACP 桥接） | Node.js 18+, [codex-acp](https://github.com/zed-industries/codex-acp) |

发送 `/start` 打开交互式 Agent 选择卡片，`/help` 查看所有命令。

---

## 设计目标

- 随时随地 Vibe Coding！
- 从第一天起就追求小巧和快速 — Bun + Rust 打造便携的、始终在线的编程伙伴。
- 一个在后台运行的上下文感知编程助手，不打断你的工作流。
- **多 Agent 支持：** Claude Code、Gemini CLI、OpenCode、Codex — 全部通过 ACP 接入，随时切换。
- **无缝设备切换：** tmux 会话跨连接持久化 — PC → 手机 → 另一台 PC → 再回来，零摩擦。
- **双轨控制：**
  - **远程终端：** 从 Web 仪表盘连接到实时 PTY，支持 tmux 会话持久化。
  - **对话式编程：** 通过 IM 发送指令；AI 异步编写、重构、审查代码。

**IM 范围（当前）：** 目前只考虑与用户的一对一（1:1）对话。群聊、广播、多聊天分发明确不在当前范围内。


---

## 快速开始

```
cd src
bun install
bun run prebuild
bun run dev
```

然后托盘菜单 → 打开 Web 仪表盘；隧道 URL 和密码会显示在终端中。

---

## 配置（settings.json）

配置文件路径：**`src/settings.json`**（从 `src/settings.json.example` 复制）。该文件已被 gitignore。

**结构：**

| 路径 | 说明 |
|------|------|
| `tunnel.provider` | `"localtunnel"`（默认）、`"ngrok"` 或 `"cloudflare"` |
| `tunnel.ngrok.auth_token` | Ngrok 认证令牌（使用 ngrok 时必填） |
| `tunnel.ngrok.domain` | 可选的 ngrok 保留域名（如 `myapp.ngrok.io`） |
| `tunnel.preview_base_url` | 可选的预览链接基础 URL（设置后覆盖 domain） |
| `channels.telegram.bot_token` | Telegram bot token，从 [@BotFather](https://t.me/BotFather) 获取；留空则禁用 Telegram |
| `channels.feishu.app_id` | 飞书应用 ID（从开放平台获取）；留空则禁用飞书 |
| `channels.feishu.app_secret` | 飞书应用密钥 |
| `channels.feishu.verbose.show_thinking` | 在飞书中显示 Agent 思考过程（默认：`false`） |
| `channels.feishu.verbose.show_tool_use` | 在飞书中显示工具调用/结果（默认：`false`） |
| `channels.telegram.verbose.show_thinking` | 在 Telegram 中显示 Agent 思考过程（默认：`false`） |
| `channels.telegram.verbose.show_tool_use` | 在 Telegram 中显示工具调用/结果（默认：`false`） |
| `tmux.detach_others` | 连接 tmux 会话时断开其他客户端（默认：`true`） |
| `working_dir` | 工作区根目录，绝对路径。未设置时默认：`{user_home}/VibeAround` |

**最小配置示例**（仅 Telegram + Localtunnel）：

```json
{
  "tunnel": { "provider": "localtunnel" },
  "channels": {
    "telegram": { "bot_token": "YOUR_TELEGRAM_BOT_TOKEN" }
  }
}
```

**飞书 + ngrok 配置：**

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

## 推荐的运行方式

**简要步骤**（从仓库根目录）：`cd src` → `bun install` → `bun run prebuild` → `bun run dev`。然后托盘菜单 → **打开 Web 仪表盘**；隧道 URL 和密码在终端中。

---

**安装路径：** 克隆仓库后，以 `src/` 目录作为工作路径：

```
VibeAround/src/
```

**环境要求：** Bun 1.3+ 和 Rust 1.78+（如需更新 Rust：`rustup update stable`）。

**配置：** 所有运行时配置（隧道、Telegram、飞书、工作目录）从 **`src/settings.json`** 读取。该文件已被 gitignore。将 `src/settings.json.example` 复制为 `src/settings.json` 并填入所需值。完整结构见上方 [配置（settings.json）](#配置settingsjson)。

**步骤（首次运行或拉取更新后）：**

1. **安装依赖** — 安装 `web`、`desktop-tray` 和 `desktop` 的工作区依赖：

```bash
cd src
bun install
```

2. **构建 Web 仪表盘和托盘 UI** — 本地服务器需要仪表盘，桌面应用需要托盘：

```bash
bun run prebuild
```

（会依次运行 `desktop-tray:build` 和 `web:build`，产出 `web/dist` 和 `desktop-tray/dist`。）

3. **运行应用** — 启动 Tauri 桌面进程（托盘、Web 服务器、隧道、IM 机器人）：

```bash
bun run dev
```

如果使用**飞书**，需要在这一步获取**隧道 URL**，然后到飞书开放平台设置 webhook。

应用运行后：

- 托盘菜单 → **打开 Web 仪表盘**，在浏览器中打开。服务器地址：

```
http://127.0.0.1:5182
```

- **隧道 URL 和密码：** 桌面应用会自动启动 Localtunnel。在**终端**中查找 `[VibeAround] Tunnel URL: https://xxx.loca.lt` 和隧道密码。也可以通过托盘菜单 → **打开隧道 URL** 打开公网仪表盘链接。

**注意：** 首次运行后，通常只需 `bun run dev`，除非修改了 `web` 或 `desktop-tray` 的代码；那时需要先 `bun run prebuild` 再 `bun run dev`。使用 `bun run build` 来生成完整的桌面应用包（Tauri 构建）。

---

### 不使用桌面应用运行（独立服务器）

如果不想运行 Tauri 桌面应用（无托盘、无隧道），可以只运行 HTTP 服务器，在本地使用 Web 仪表盘：

1. 在 `src/` 下运行 `bun run prebuild`，确保 `web/dist` 存在。
2. 启动服务器：

```bash
bun run server:dev
```

仪表盘地址：`http://127.0.0.1:5182`。独立服务器**不会**启动 Localtunnel 或 Telegram 机器人；仅供本地使用（如无头机器或只需 Web UI 的场景）。

---

## tmux 与无缝设备切换

VibeAround 支持挂载 tmux 会话，让你可以随身带走未完成的工作。在一台设备上开始编程会话，断开后在另一台设备上继续 — Web 仪表盘、另一个浏览器、或通过 SSH 的原生终端客户端都行。

### 工作原理

在 Web 仪表盘创建会话时，可以选择将其挂载到宿主机的 tmux 会话。关闭浏览器标签或断网后，tmux 会话在后台继续运行。从任何设备重新连接，一切都在原来的地方。

默认情况下，连接会话会断开其他客户端（`tmux attach -d`）。这提供了干净的单查看者语义 — 在手机上打开仪表盘，之前的浏览器标签会优雅断开。如需允许多个查看者同时连接同一会话，在 `settings.json` 中将 `tmux.detach_others` 设为 `false`。

### tmux 面板和分屏操作

Web 终端支持以下 tmux 风格的面板和分屏操作。前缀键是 **Ctrl+b**（按下 Ctrl+b，松开，再按下面的键）。

| 操作 | 快捷键 |
|------|--------|
| **垂直分屏** | **Ctrl+b** 然后 **%** |
| **水平分屏** | **Ctrl+b** 然后 **"** |
| **移动焦点**（面板导航） | **Ctrl+b** 然后 **↑** **↓** **←** **→**（方向键） |
| **循环切换面板** | **Ctrl+b** 然后 **o**（切换到下一个面板） |
| **显示面板编号** | **Ctrl+b** 然后 **q**（编号闪现；按对应数字跳转） |
| **关闭当前面板** | **Ctrl+b** 然后 **x** |

### 推荐：iTerm2 + tmux -CC 集成

在 Mac 上使用时，推荐 [iTerm2](https://iterm2.com) 的原生 tmux 集成模式。iTerm2 不会在终端模拟器内渲染 tmux，而是将每个 tmux 窗口/面板映射为原生 iTerm2 标签/分屏 — 原生滚动、原生复制粘贴、原生快捷键，同时底层仍由持久化的 tmux 会话支撑。

**快速开始：**

```bash
# SSH 到你的 VibeAround 主机，用 tmux -CC 连接
ssh your-host -t "tmux -CC attach -t my-session"
```

或在已有的 SSH 会话中：

```bash
tmux -CC attach -t my-session
```

iTerm2 会自动检测 `-CC` 标志并切换到原生集成模式。

**对 VibeAround 的意义：**

- 在公司 PC 上通过 Web 仪表盘开始 Vibe Coding 会话。
- 在路上，通过手机的 Web 仪表盘或 Telegram 查看进度、发送快速指令。
- 回到家，用 iTerm2 通过 `tmux -CC` SSH 到同一台机器，获得完全原生的终端体验 — 同一个会话，同一个状态，零上下文丢失。

工作流：**PC → 手机 → 另一台 PC → 再回来**，全程无缝。

---

## 路线图

- [x] 远程 PTY 终端 + Web 仪表盘，多会话，tmux 持久化
- [x] 隧道支持（Ngrok、Localtunnel、Cloudflare）公网 URL 访问
- [x] Telegram 机器人集成
- [x] 飞书机器人集成（webhook + 交互卡片）
- [x] 多 Agent ACP 接入：Claude Code、Gemini CLI、OpenCode、Codex
- [x] 通过 `/cli_` 命令和 `/start` 卡片切换 Agent
- [x] 缓冲流式输出 + 消息反应（processing / done）
- [x] 按渠道配置 verbose（show_thinking、show_tool_use）
- [ ] 工作区：通过 IM 或 Web 仪表盘切换和管理项目文件夹
- [ ] Agent 设置：模型选择、API 密钥、每个 Agent 的生成选项
- [ ] 技能与上下文：自定义流程、提示词模板、项目规则
- [ ] IM 多账号：将特定聊天绑定到特定工作区
- [ ] 历史记录：SQLite 支持的对话和任务日志
- [ ] 端口发现：检测新的开发服务器并自动建立隧道
- [ ] 更多 IM：Discord、Slack
- [ ] 插件：社区适配器、日志清理器、工作流插件
- [ ] 安全与路由：基于意图的 Agent 选择、Git Sentinel 快照

---

## 项目状态与贡献

**VibeAround 目前处于早期概念验证（POC）阶段。**

源代码以 MIT 许可证开放，旨在透明、教育和分享愿景。**目前不接受 Pull Request 和功能请求。** 架构、核心组件和 API 正在快速变化，可能有破坏性更改；目的是避免你在可能与内部路线图冲突的 PR 上浪费时间。项目稳定后（Phase 2/3）可能会开放社区贡献。

**关于 AI 生成（Dogfooding）：** VibeAround 的大量代码是使用 AI 编程工具生成的（正是我们所倡导的 "Vibe Coding" 工作流）。我们认为这证明了 AI 辅助编排的力量。

欢迎 fork 项目、探索代码、自由实验。

## 许可证

本项目基于 [MIT 许可证](LICENSE) 开源。
