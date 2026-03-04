<div align="center">

# VibeAround — 无处不在的 Vibe 编程！

[English](README.md) | [简体中文](https://www.google.com/search?q=README_CN.md)

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

**VibeAround** 是一款运行在您本地设备上的常驻 Vibe 编程工具。它提供了两种与 AI 编程智能体交互的方式——基于浏览器的远程终端和即时通讯（IM）机器人（Telegram、飞书）——让您可以随时随地进行 Vibe 编程。

**四个 AI 智能体，统一的交互界面** —— Claude Code、Gemini CLI、OpenCode 和 Codex，均通过 [Agent Client Protocol (ACP)](https://agentclientprotocol.com/) 连接。您可以通过 IM 命令随时切换智能体。

**基于浏览器的远程终端** —— 直接从 Web 控制台打开 Shell、附加到 tmux 会话，或快速启动这四个智能体中的任何一个。会话跨设备保持不变——在电脑上开始任务，在手机上查看进度。

## 界面截图

桌面端与移动端 Web 控制台 —— 同一会话，多端同步。

| 桌面端 | 移动端 |
| --- | --- |
| <img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/screenshots/pc.webp" width="720" alt="桌面端 VibeAround Web 控制台" /> | <img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/screenshots/mobile-claude.webp" width="200" alt="移动端 VibeAround Web 控制台" /> |

## 支持的智能体

VibeAround 通过 [Agent Client Protocol (ACP)](https://agentclientprotocol.com/) 连接 AI 编程智能体。您可以随时通过 IM 命令或交互式卡片切换当前使用的智能体。

| 智能体 | 命令 | 连接方式 | 前置要求 |
| --- | --- | --- | --- |
| Claude Code | `/cli_claude` | 进程内 ACP 桥接 → `claude` CLI | [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code) |
| Gemini CLI | `/cli_gemini` | `gemini --experimental-acp`（原生 ACP） | [Gemini CLI](https://github.com/google-gemini/gemini-cli) |
| OpenCode | `/cli_opencode` | `opencode acp`（原生 ACP） | [OpenCode](https://github.com/opencode-ai/opencode) |
| Codex | `/cli_codex` | `npx @zed-industries/codex-acp`（ACP 桥接） | Node.js 18+, [codex-acp](https://github.com/zed-industries/codex-acp) |

发送 `/start` 可以获取交互式智能体选择卡片，或发送 `/help` 查看所有可用命令。

**核心目标**

- 随时随地 Vibe 编程！
- 自发布起即保持轻量极速 —— 结合 Bun 与 Rust，打造便携、全天候在线的 Vibe 编程伴侣。
- 在后台默默运行的具备上下文感知的编程助手，绝不打断您的现有工作流。
- **多智能体支持：** Claude Code、Gemini CLI、OpenCode、Codex —— 全部基于 ACP 接入，支持随时无缝切换。
- **设备无缝切换：** tmux 会话跨连接持久化 —— 电脑 → 手机 → 另一台电脑 → 随时切回，零摩擦体验。
- **双轨**控制模式：

- **远程终端：** 从 Web 控制台附加到实时 PTY（伪终端），并支持 tmux 会话持久化。
- **对话式 Vibe 编程：** 通过 IM（即时通讯）发送指令；AI 会在后台异步编写、重构和审查代码。

**IM 功能范围（当前）：** 在可预见的未来，我们仅考虑与用户的**一对一 (1:1) 对话**。广播、群组消息以及多聊天分发等功能明确不在当前讨论范围内，这些将在后续阶段解决。

## 快速开始

```
cd src
bun install
bun run prebuild
bun run dev

```

随后点击系统托盘菜单 → **Open Web Dashboard**（打开 Web 控制台）；内网穿透（Tunnel）的 URL 及密码将显示在终端日志中。

## 配置说明 (settings.json)

配置文件路径：**`src/settings.json`**（请从 `src/settings.json.example` 复制创建）。该文件已被 Git 忽略，不会被提交。

**结构说明：**

| 配置路径 | 描述 |
| --- | --- |
| `tunnel.provider` | `"localtunnel"`（默认）、`"ngrok"` 或 `"cloudflare"` |
| `tunnel.ngrok.auth_token` | Ngrok 认证 Token（若提供商为 ngrok 则必填） |
| `tunnel.ngrok.domain` | 可选的保留 ngrok 域名（例如：`myapp.ngrok.io`） |
| `tunnel.preview_base_url` | 可选的预览链接基础 URL（设置后将覆盖 domain 配置） |
| `channels.telegram.bot_token` | 来自 [@BotFather](https://t.me/BotFather) 的 Telegram 机器人 Token；若留空则禁用 Telegram 机器人 |
| `channels.telegram.streaming.enable` | 启用通过消息编辑实现的流式输出（默认：`false`） |
| `channels.telegram.streaming.edit_interval_ms` | 流式输出时 edit_message 调用的最小间隔毫秒数（默认：`100`） |
| `channels.feishu.app_id` | 飞书应用 ID（来自开放平台）；若留空则禁用飞书机器人 |
| `channels.feishu.app_secret` | 飞书应用密钥 (App Secret) |
| `channels.feishu.streaming.enable` | 启用通过消息编辑实现的流式输出（默认：`false`） |
| `channels.feishu.streaming.edit_interval_ms` | 流式输出时 edit_message 调用的最小间隔毫秒数（默认：`200`） |
| `channels.feishu.verbose.show_thinking` | 在飞书中显示智能体的思考过程块（默认：`false`） |
| `channels.feishu.verbose.show_tool_use` | 在飞书中显示工具调用记录及结果（默认：`false`） |
| `channels.telegram.verbose.show_thinking` | 在 Telegram 中显示智能体的思考过程块（默认：`false`） |
| `channels.telegram.verbose.show_tool_use` | 在 Telegram 中显示工具调用记录及结果（默认：`false`） |
| `tmux.detach_others` | 附加至 tmux 会话时，分离其他已连接的客户端（默认：`true`） |
| `working_dir` | 任务工作区的根目录，需为绝对路径。未设置时默认为：`{user_home}/VibeAround` |
| `default_agent` | 首条消息时默认启动的 Agent：`claude`、`gemini`、`opencode`、`codex`（默认：`claude`） |

**最简配置示例**（仅启用 Telegram + Localtunnel）：

```
{
  "tunnel": { "provider": "localtunnel" },
  "channels": {
    "telegram": { "bot_token": "YOUR_TELEGRAM_BOT_TOKEN" }
  }
}

```

**启用飞书与 ngrok 的配置示例：**

```
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

**启用流式输出的配置示例：**

```
{
  "tunnel": {
    "provider": "ngrok",
    "ngrok": {
      "auth_token": "YOUR_NGROK_AUTH_TOKEN",
      "domain": "your-reserved.ngrok.io"
    }
  },
  "channels": {
    "telegram": {
      "bot_token": "YOUR_TELEGRAM_BOT_TOKEN",
      "streaming": { "enable": true, "edit_interval_ms": 100 }
    },
    "feishu": {
      "app_id": "YOUR_FEISHU_APP_ID",
      "app_secret": "YOUR_FEISHU_APP_SECRET",
      "streaming": { "enable": true, "edit_interval_ms": 200 }
    }
  }
}

```

## 推荐的安装与运行方式

**简而言之（在仓库根目录下执行）：** `cd src` → `bun install` → `bun run prebuild` → `bun run dev`。随后点击系统托盘菜单 → **Open Web Dashboard**；内网穿透 URL 和密码将显示在终端中。

**安装路径：** 克隆本仓库，随后将 `src/` 目录作为您的工作路径：

```
VibeAround/src/

```

**环境要求：** Bun 1.3+ 以及 Rust 1.78+（如果需要，请使用 `rustup update stable` 更新 Rust）。

**配置：** 所有的运行时配置（内网穿透、Telegram、飞书、工作目录）都读取自 **`src/settings.json`**。该文件已被 Git 忽略。请将 `src/settings.json.example` 复制一份命名为 `src/settings.json` 并填入您所需的值。完整结构请参阅上方的[配置说明 (settings.json)](https://www.google.com/search?q=%23%E9%85%8D%E7%BD%AE%E8%AF%B4%E6%98%8E-settingsjson)。

**操作步骤（首次运行或拉取更新后）：**

1. **安装依赖** —— 此操作将为 `web`、`desktop-tray` 和 `desktop` 安装工作区依赖：

```
cd src
bun install

```

1. **构建 Web 控制台和托盘 UI** —— 此步骤必不可少，以确保本地服务器能提供控制台服务，并且桌面端程序能够加载托盘菜单：

```
bun run prebuild

```

（该命令会依次执行 `desktop-tray:build` 和 `web:build`，并生成 `web/dist` 和 `desktop-tray/dist` 产物目录。）

1. **运行应用** —— 启动 Tauri 桌面端进程（包含系统托盘、Web 服务器、内网穿透和 IM 机器人）：

```
bun run dev

```

如果您使用**飞书**，需要在此步骤获取**内网穿透 URL (tunnel URL)**，之后才能在飞书开放平台中配置 Webhook。

应用启动后：

- 使用托盘菜单 → 点击 **Open Web Dashboard** 以在浏览器中打开控制台。本地服务器的默认地址为：

```
[http://127.0.0.1:5182](http://127.0.0.1:5182)

```

- **内网穿透 URL 与密码：** 桌面端应用会自动启动 Localtunnel。请留意终端输出的类似于 `[VibeAround] Tunnel URL: https://xxx.loca.lt` 的信息，以及穿透密码（或获取密码的链接）。您也可以直接使用托盘菜单 → 点击 **Open tunnel URL** 来打开可通过公网访问的控制台链接。

**注意：** 首次运行后，日常开发和使用只需执行 `bun run dev` 即可；但如果您修改了 `web` 或 `desktop-tray` 目录下的代码，请在运行前再次执行 `bun run prebuild`。当您需要打包出完整的桌面端应用程序（Tauri 构建）时，请使用 `bun run build`。

### 无桌面端运行（独立服务器模式）

如果您不想运行 Tauri 桌面端程序（不需要托盘和内网穿透功能），您可以仅启动 HTTP 服务器，在本地使用 Web 控制台：

1. 在 `src/` 目录下，运行 `bun run prebuild`，确保生成了 `web/dist`。
1. 启动服务器：

```
bun run server:dev

```

控制台的访问地址将是 `http://127.0.0.1:5182`。独立服务器**不会**启动 Localtunnel 或 Telegram 机器人；它仅供本地网络使用（例如，运行在无头服务器上，或当您仅仅需要 Web UI 时）。

## tmux 与无缝设备切换

VibeAround 支持附加（attach）到 tmux 会话，让您可以随时带走未完成的工作。在一台设备上开始编程会话，断开连接，然后在另一台设备上继续——无论是在 Web 控制台、其他浏览器，还是通过 SSH 连接的原生终端客户端。

### 它的工作原理

当您在 Web 控制台中创建一个会话时，可以选择将其附加到宿主机的 tmux 会话中。如果您关闭了浏览器标签页或网络断开，tmux 会话仍将在后台继续运行。从任何设备重新连接后，您都能立刻回到刚才离开时的状态。

默认情况下，附加到一个会话会断开其他客户端的连接（`tmux attach -d`）。这保证了清晰的“单一查看者”逻辑——在手机上打开控制台，之前电脑上的浏览器标签页会自动优雅地断开。如果您允许多个查看者共享同一个会话，可以在 `settings.json` 中将 `tmux.detach_others` 设置为 `false`。

### Tmux 窗格与分屏操作

Web 终端支持以下类似 tmux 的窗格与分屏操作。前缀键是 **Ctrl+b**（先按下 Ctrl+b，松开后，再按以下列表中的按键）。

| 操作 | 快捷键 |
| --- | --- |
| **垂直分屏** | **Ctrl+b** 随后按 **%** |
| **水平分屏** | **Ctrl+b** 随后按 **"** |
| **移动焦点**（切换窗格） | **Ctrl+b** 随后按 **↑** **↓** **←** **→**（方向键） |
| **循环切换窗格** | **Ctrl+b** 随后按 **o**（跳转至下一个窗格） |
| **显示窗格编号** | **Ctrl+b** 随后按 **q**（编号会在屏幕上短暂闪现；按下对应的数字即可跳转至该窗格） |
| **关闭当前窗格** | **Ctrl+b** 随后按 **x** |

### 强烈推荐：使用支持 tmux -CC 集成的 iTerm2

对于 Mac 用户，为了获得最佳体验，我们推荐使用 [iTerm2](https://iterm2.com) 原生的 tmux 集成模式。iTerm2 不会在终端模拟器内部渲染 tmux 界面，而是将每个 tmux 窗口/窗格映射为原生的 iTerm2 标签页/分屏——这为您提供了原生的历史滚动、原生的复制粘贴以及原生的键盘快捷键体验，同时依然由远程主机上持久化的 tmux 会话作为后端支撑。

**快速开始：**

```
# 通过 SSH 登录您的 VibeAround 宿主机，并使用 tmux -CC 附加会话
ssh your-host -t "tmux -CC attach -t my-session"

```

或者在已有的 SSH 会话中执行：

```
tmux -CC attach -t my-session

```

iTerm2 会自动检测到 `-CC` 标志，并无缝切换至其原生集成模式。

**为什么这对 VibeAround 来说很重要：**

- 在公司电脑上，通过 Web 控制台开启一个 Vibe 编程会话。
- 在通勤路上，通过手机上的 Web 控制台或 Telegram 查看进度，发送简短的指令。
- 回到家中，在 Mac 上通过 iTerm2 执行带有 `tmux -CC` 的 SSH 命令登录同一台机器，获得完全原生的终端体验——同一个会话，同样的状态，零上下文丢失。

整体工作流即是：**电脑 → 手机 → 另一台电脑 → 随时切回**，全程无缝衔接。

## 开发路线图

- [x] 带有 Web 控制台的远程 PTY 终端，支持多会话和 tmux 持久化
- [x] 内网穿透支持（Ngrok、Localtunnel、Cloudflare）以提供公网 URL 访问
- [x] Telegram 机器人集成
- [x] 飞书机器人集成（Webhook + 交互式消息卡片）
- [x] 基于 ACP 的多智能体支持：Claude Code、Gemini CLI、OpenCode、Codex
- [x] 通过 `/cli_` 命令和 `/start` 卡片自由切换智能体
- [x] 带有状态反馈（处理中/已完成）的缓冲流式输出
- [x] 按渠道配置的详细输出项（显示思考过程、显示工具使用情况）
- [ ] 工作区管理：通过 IM 或 Web 控制台切换和管理项目文件夹
- [ ] 智能体设置：为每个智能体独立配置模型选择、API 密钥和生成选项
- [ ] 技能与上下文：自定义流程、提示词模板、项目级规则
- [ ] IM 多账户支持：将特定聊天绑定到特定工作区
- [ ] 历史记录：基于 SQLite 的对话与任务日志
- [ ] 端口发现：自动检测新的开发服务器并为其自动建立内网穿透
- [ ] 更多消息平台支持：Discord、Slack
- [ ] 插件系统：社区适配器、日志净化器、工作流插件
- [ ] 安全与路由：基于意图的智能体选择，Git 哨兵自动快照

## 项目状态与贡献指南

**VibeAround 目前正处于早期的概念验证 (POC) 阶段。**

本项目源代码基于 MIT 协议开源，旨在保持透明、促进技术交流并分享产品愿景。**我们当前暂不接受 Pull Request (PR) 和新功能请求 (Feature Request)。** 架构、核心组件和 API 目前迭代迅速，并可能包含破坏性更新；这样做的目的是避免您将宝贵的时间浪费在可能与内部路线图冲突的 PR 上。随着项目趋于稳定（进入第 2/3 阶段），我们将在后续开放社区贡献。

**关于 AI 生成代码（内部实践/Dogfooding）：** VibeAround 的很大一部分代码正是使用 AI 编程工具生成的（这正是我们所倡导的 "Vibe 编程" 工作流）。我们相信，这有力地证明了 AI 辅助编排的强大威力。

欢迎随时 Fork 本项目，探索代码并在您自己的环境中进行实验。

## 开源协议

本项目基于 [MIT 协议](https://www.google.com/search?q=LICENSE) 开源。