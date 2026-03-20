<div align="center">

# VibeAround — 无处不在的 Vibe 编程！

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

**IM 功能范围（当前）：** 一对一对话（[飞书插件](https://github.com/jazzenchen/vibearound-plugin-feishu)、[Telegram 插件](https://github.com/jazzenchen/vibearound-plugin-telegram)）。

## 快速开始

```
cd src
bun install
bun run prebuild
bun run dev
```

随后点击系统托盘菜单 → **Open Web Dashboard**（打开 Web 控制台）；内网穿透（Tunnel）的 URL 及密码将显示在终端日志中。

首次启动时，桌面端会进入 **onboarding wizard**，引导你完成智能体选择、IM 令牌和隧道配置。向导会把配置写入 `~/.vibearound/settings.json`。

> **配置路径变更：** 当前版本从用户目录 `~/.vibearound/settings.json` 读取配置，不再直接使用仓库内的 `src/settings.json`。仓库内文件仅作为开发种子配置。

### IM 插件安装

VibeAround 将 IM 支持通过插件实现。官方插件源码在独立仓库：

- [vibearound-plugin-telegram](https://github.com/jazzenchen/vibearound-plugin-telegram)
- [vibearound-plugin-feishu](https://github.com/jazzenchen/vibearound-plugin-feishu)

从官方仓库构建插件：

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

安装到运行时插件目录：

- `~/.vibearound/plugins/telegram`
- `~/.vibearound/plugins/feishu`

每个 IM 插件目录下都需要包含 `dist/main.js`。Host 会根据 `~/.vibearound/settings.json` 中启用的 IM 名加载对应插件。

详细的安装步骤、配置说明和独立服务器模式，请参阅 Wiki 中的[安装与运行指南](https://github.com/jazzenchen/VibeAround/wiki/Setup-Guide-CN)。

---

## 📖 Wiki

完整的配置文档和使用指南已移至 [Wiki](https://github.com/jazzenchen/VibeAround/wiki)：

- [安装与运行指南](https://github.com/jazzenchen/VibeAround/wiki/Setup-Guide-CN) — 安装、配置与运行
- [Channel 配置指南](https://github.com/jazzenchen/VibeAround/wiki/Channel-Configuration-CN) — Telegram 和飞书机器人配置
- [Tunnel 配置指南](https://github.com/jazzenchen/VibeAround/wiki/Tunnel-Configuration-CN) — Localtunnel、Ngrok、Cloudflare 隧道配置
- [tmux 使用指南](https://github.com/jazzenchen/VibeAround/wiki/Tmux-Guide-CN) — tmux 会话、分屏操作与无缝设备切换

---

## 开发路线图

- [x] 带有 Web 控制台的远程 PTY 终端，支持多会话和 tmux 持久化
- [x] 内网穿透支持（Ngrok、Localtunnel、Cloudflare）以提供公网 URL 访问
- [x] Telegram 机器人集成
- [x] 飞书机器人集成（Webhook + 交互式消息卡片）
- [x] 基于 ACP 的多智能体支持：Claude Code、Gemini CLI、OpenCode、Codex
- [x] 通过 `/cli_` 命令和 `/start` 卡片自由切换智能体
- [x] 带有状态反馈（处理中/已完成）的缓冲流式输出
- [x] 按渠道配置的详细输出项（显示思考过程、显示工具使用情况）
- [x] Desktop onboarding wizard：首次运行完成智能体、渠道与隧道设置
- [ ] 下一步：CLI 插件化（让各类 Agent CLI 适配器以插件方式加载）
- [ ] 工作区管理：通过 IM 或 Web 控制台切换和管理项目文件夹
- [ ] 智能体设置：为每个智能体独立配置模型选择、API 密钥和生成选项
- [ ] 技能与上下文：自定义流程、提示词模板、项目级规则
- [ ] IM 多账户支持：将特定聊天绑定到特定工作区
- [ ] 历史记录：基于 SQLite 的对话与任务日志
- [ ] 端口发现：自动检测新的开发服务器并为其自动建立内网穿透
- [ ] 更多消息平台支持：Discord、Slack
- [ ] 插件系统：社区适配器、日志净化器、工作流插件
- [ ] 安全与路由：基于意图的智能体选择，Git 哨兵自动快照

---

## 项目状态与贡献指南

**VibeAround 目前正处于早期的概念验证 (POC) 阶段。**

本项目源代码基于 MIT 协议开源，旨在保持透明、促进技术交流并分享产品愿景。**我们当前暂不接受 Pull Request (PR) 和新功能请求 (Feature Request)。** 架构、核心组件和 API 目前迭代迅速，并可能包含破坏性更新；这样做的目的是避免您将宝贵的时间浪费在可能与内部路线图冲突的 PR 上。随着项目趋于稳定（进入第 2/3 阶段），我们将在后续开放社区贡献。

**关于 AI 生成代码（内部实践/Dogfooding）：** VibeAround 的很大一部分代码正是使用 AI 编程工具生成的（这正是我们所倡导的 "Vibe 编程" 工作流）。我们相信，这有力地证明了 AI 辅助编排的强大威力。

欢迎随时 Fork 本项目，探索代码并在您自己的环境中进行实验。

---

## 更新日志（近期）

### 会话持久化修复

此前，每一轮对话（用户消息 → 智能体回复）都会创建一个**新的** JSONL 会话文件，原因是 `SessionWriter` 在 `TurnComplete` 时被丢弃。现在消息中心（message hub）会记住每个智能体上次的会话文件路径，并在后续轮次中重新打开同一文件进行追加写入，从而将连续对话持久化到单个会话文件中。`/new` 命令仍会按预期开启全新会话。

### Gemini ACP / MCP 工具发现

调查发现，Gemini CLI 在 `--experimental-acp` 模式下运行时，**不会**从 `.gemini/settings.json` 文件中发现 MCP 服务器。MCP 服务器必须通过 ACP 的 `session/new` 请求中的 `mcpServers` 数组参数注入。这是 Gemini 无法获取 `dispatch_task` 工具的根本原因。修复方案：需要在 `acp_session_loop` 中将 MCP 服务器配置传入 `NewSessionRequest`。

---

## 开源协议

本项目基于 [MIT 协议](LICENSE) 开源。
