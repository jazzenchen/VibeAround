<div align="center">

<img src="Logo.png" alt="VibeAround logo" width="96" />

# VibeAround

**让你的 AI 编程 Agent 一直在身边。**

从一个地方启动 Claude Code、Codex CLI、Gemini CLI、Pi Agent 等工具：并排运行，保持连接，随处可达，并且随时可以在 Web、移动端和消息平台继续工作。

[下载](https://github.com/jazzenchen/VibeAround/releases/latest) | [演示](https://youtu.be/6kxNKTMz-AM) | [Wiki](https://github.com/jazzenchen/VibeAround/wiki) | [Discord](https://discord.gg/KsJWkY64GN) | [English](README.md)

<p align="center">
  <img src="https://img.shields.io/badge/Rust-1.82+-000?style=flat-square&logo=rust&logoColor=fff" alt="Rust" />
  <img src="https://img.shields.io/badge/Tauri-2.10-24C8DB?style=flat-square&logo=tauri&logoColor=fff" alt="Tauri" />
  <img src="https://img.shields.io/badge/React-19-61DAFB?style=flat-square&logo=react&logoColor=000" alt="React" />
  <img src="https://img.shields.io/badge/ACP-local%20agents-111?style=flat-square" alt="ACP local agents" />
  <img src="https://img.shields.io/badge/License-MIT-blue?style=flat-square" alt="License: MIT" />
</p>

</div>

VibeAround 把执行留在本机。Claude Code、Codex CLI、Gemini CLI、Pi Agent 等工具仍然运行在你的电脑、项目目录、shell、文件系统、凭证和权限环境里。VibeAround 负责给这些本地会话提供统一入口：桌面 Launch、浏览器 Dashboard、移动端友好的 Web Chat、Web Terminal、IM 频道和短时预览。

Profile 也可以接第三方 provider API。VibeAround 可以维护模型列表、自定义模型名、context metadata，并在 OpenAI Responses、OpenAI Chat Completions、Anthropic Messages、Gemini Generate Content 等 API 形态之间做 bridge，让 Agent 可以使用 DeepSeek、Kimi、DashScope、xAI/Grok、MiniMax、NVIDIA NIM，或者你自己的 OpenAI-compatible endpoint。

这里的远程访问不必依赖 Agent 厂商自己的订阅远程能力。Agent 继续在本机运行，VibeAround 通过自己的 tunnel 和 channel 层，让同一个本地 session 可以从 Web、移动端或消息平台继续访问。底层 CLI 支持 API key/provider 配置时，也可以走普通 provider API 计费，而不是被单一 Agent 订阅绑定。

## 解决什么问题

本地编程 Agent 很强，但会话、模型配置和运行状态通常散落在不同终端里。VibeAround 把本机作为真实运行环境，同时提供多个稳定入口。

| 问题 | VibeAround 提供 |
|---|---|
| Agent CLI 和模型配置太多 | 一个桌面 Launch 页面管理 Agent、Profile、Workspace、Session 和 Terminal |
| 想使用 Agent 默认列表之外的 provider | 第三方 provider API key、模型列表、自定义模型名、context metadata 和 bridge route |
| Provider API 形态和 Agent 不匹配 | 在 OpenAI Responses、Chat Completions、Anthropic Messages、Gemini Generate Content 之间做本地 bridge |
| 不想每次重新开始会话 | Workspace/Session 选择、host session 发现，以及 handover 命令 |
| 想远程访问，但不想把执行搬到厂商云端 | Web Chat、Web Terminal、IM 频道、tunnel 和连接本地 session 的短时预览链接 |
| 希望配置可复现 | 首次引导、频道插件安装、MCP/Skill 注入和本地设置管理 |

## 产品地图

| 区域 | 作用 |
|---|---|
| **Launch** | 直接启动 Agent，或通过 Provider Profile 启动；同一处选择 terminal、workspace 和 new/resume session |
| **Profiles** | 保存 provider 凭证、模型列表、bridge route、API 形态转换和 provider 选项 |
| **Workspaces** | 围绕本地项目目录组织启动上下文、会话历史和工作状态 |
| **Web Dashboard** | 在浏览器中打开 Web Chat、Web Terminal、预览、运行状态和本地会话视图 |
| **IM Channels** | 通过 Telegram、飞书/Lark、Discord、Slack、微信、钉钉、企业微信、QQ Bot 私聊本地 Agent |
| **Previews** | 为本地 dev server、Markdown、HTML 生成短时效鉴权预览链接 |
| **Settings** | 管理 Agent、插件、Tunnel、Proxy、IM verbosity、语言和更新检查 |

## 产品截图

<p align="center">
  <img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/documents/v0.6.5/current-app/app-launch-cn.webp" width="88%" alt="VibeAround 桌面 Launch 页面，包含 Agent、Profile、Workspace、Session 与启动按钮" />
</p>

<p align="center">
  <img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/documents/v0.6.5/screenshots/web-chat.webp" width="49%" alt="VibeAround Web Chat 与浏览器 Dashboard" />
  <img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/documents/v0.6.5/screenshots/model-bridge-terminals.webp" width="49%" alt="通过模型 Bridge Profile 启动的 Codex 终端" />
</p>

<p align="center">
  <img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/documents/v0.6.5/current-app/app-provider-catalog-cn.webp" width="49%" alt="Provider catalog 与支持的 API 形态" />
  <img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/documents/v0.6.5/current-app/app-profile-form-cn.webp" width="49%" alt="Provider Profile 表单、API 类型与模型设置" />
</p>

## 快速开始

1. 下载适合你系统的最新桌面包。
2. 打开 VibeAround，完成首次引导。
3. 启用你常用的 Agent CLI。
4. 如果需要模型路由，添加一个或多个 Provider Profile。
5. 在 Launch 里选择 Agent、Profile、Terminal、Workspace 和 Session。
6. 从桌面、Web Chat、Web Terminal 或配置好的 IM 频道继续使用。

更详细的文档见 [Wiki](https://github.com/jazzenchen/VibeAround/wiki)。

## 下载

最新版本：[VibeAround v0.6.6](https://github.com/jazzenchen/VibeAround/releases/tag/v0.6.6)。

| 平台 | 推荐下载 |
|---|---|
| macOS Apple Silicon | [VibeAround_0.6.6_arm64.dmg](https://github.com/jazzenchen/VibeAround/releases/download/v0.6.6/VibeAround_0.6.6_arm64.dmg) |
| Windows x64 | [Setup EXE](https://github.com/jazzenchen/VibeAround/releases/download/v0.6.6/VibeAround_0.6.6_x64-setup.exe)、[MSI](https://github.com/jazzenchen/VibeAround/releases/download/v0.6.6/VibeAround_0.6.6_x64_en-US.msi) 或 [免安装 ZIP](https://github.com/jazzenchen/VibeAround/releases/download/v0.6.6/VibeAround-win-0.6.6-portable.zip) |
| Linux x64 | [AppImage](https://github.com/jazzenchen/VibeAround/releases/download/v0.6.6/VibeAround_0.6.6_amd64.AppImage) 或 [deb](https://github.com/jazzenchen/VibeAround/releases/download/v0.6.6/VibeAround_0.6.6_amd64.deb) |

Windows 和 Linux 包由 GitHub Actions 构建。macOS 当前发布 Apple Silicon 版本。

## 核心概念

### Agent

Agent 是 VibeAround 启动或接入的编程 CLI。Agent 运行在本机，优先通过 stdio/ACP 风格 adapter 通信。

| Agent | 启动 | Resume / handover | Profile 路由 |
|---|---:|---:|---:|
| Claude Code | 支持 | 支持 | 支持 |
| Codex CLI | 支持 | 支持 | 支持 |
| Pi | 支持 | 支持 | 支持 |
| Gemini CLI | 支持 | 支持 | 支持 |
| OpenCode | 支持 | 部分支持 | 支持 |
| Cursor CLI | 直连 | 支持 | 不支持 |
| Kiro CLI | 直连 | 支持 | 不支持 |
| Qwen Code | 直连 | 支持 | 不支持 |

### Profile

Profile 是保存好的 provider 连接。它可以只是“使用 CLI 已登录态”，也可以是“用某个 DeepSeek bridge route 启动 Codex，并向 Codex/Claude 暴露多个模型选项”。

Profile 可以包含：

- provider API key 和 base URL
- endpoint / API kind 选择
- OpenAI Responses、OpenAI Chat、Anthropic Messages、Gemini route 信息
- 模型列表、fake ID、上游模型映射和 context-window metadata
- DeepSeek reasoning 等 provider 特殊选项
- API bridge 流量的 proxy opt-in

### Workspace 和 Session

Workspace 是本地项目目录。Session 是围绕该 workspace 的 Agent 对话或终端运行。Launch 默认可以启动新会话，也可以在 Agent 支持时 resume 已有 host session。

### 本地 API Bridge

VibeAround 可以为每个 Profile 暴露 loopback endpoint，在常见模型 API 形态之间转换：

| API 形态 | 常见 endpoint |
|---|---|
| OpenAI Responses | `/v1/responses` |
| OpenAI Chat Completions | `/v1/chat/completions` |
| Anthropic Messages | `/v1/messages` |
| Gemini Generate Content | `/v1beta/models/{model}:generateContent` |

Bridge 由 [va-ai-api-bridge](https://github.com/jazzenchen/va-ai-api-bridge) 提供能力。

## 支持的 Provider

内置 Provider 覆盖常见官方和兼容接口。只要你的 provider 支持对应 API 形态，也可以使用 Custom endpoint。

| Provider | 说明 |
|---|---|
| DeepSeek | OpenAI-compatible 和 bridge route，支持模型 alias 与 Claude suffix 归一化 |
| 阿里云百炼 | Coding Plan 和 Token Plan endpoint |
| Moonshot / Kimi | OpenAI-compatible 和 Anthropic-style bridge flow |
| MiniMax | OpenAI-compatible 和 Anthropic-style bridge flow |
| Xiaomi MiMo | Token Plan 与多区域 endpoint，处理 provider 特有返回形态 |
| xAI / Grok | Responses 和 Chat 形态 |
| NVIDIA NIM | OpenAI-compatible Chat Completions |
| Z.AI / GLM | 内置兼容 endpoint |
| Google Gemini | 原生 Gemini API profile |
| OpenRouter | OpenAI-compatible profile |
| Azure OpenAI | Responses 和 Chat deployment 路由 |
| Custom endpoint | 自定义 base URL、headers、models 和 API kinds |

## IM 频道

频道插件是独立的 Node.js package，由 VibeAround 安装和管理。

| 频道 | 配置方式 | 常见用途 |
|---|---|---|
| Telegram | BotFather Token | 个人 bot 和移动端对话 |
| 飞书 / Lark | 应用凭证 | 团队 IM 和企业 bot |
| Discord | Bot Token | Server 和 DM 工作流 |
| Slack | Bot/App Token + Socket Mode | Workspace DM 工作流 |
| 微信 | OpenClaw-compatible bridge 二维码登录 | 中文个人聊天 |
| 钉钉 | Stream API 凭证 | 企业聊天 |
| 企业微信 | WebSocket bot 凭证 | 企业微信工作流 |
| QQ Bot | Guild bot 凭证 | QQ bot 工作流 |

## Web、Terminal 和预览

Dashboard 提供浏览器优先的本地工作入口：

- Web Chat：Agent 对话、附件、thinking/tool 显示、归档会话和 resume workspace session。
- Web Terminal：本地 PTY session，适合移动端访问，也适合 tmux 流程。
- Previews：为 dev server、Markdown、HTML 生成短时效链接。
- Status：查看 Agent、Channel、Tunnel 和 runtime 健康状态。

## 安全模型

VibeAround 默认是 local-first：

- daemon 默认只监听 loopback，除非你显式开启 tunnel。
- Dashboard API 和 WebSocket 路由需要本地 auth token。
- 公网 tunnel URL 需要浏览器配对。
- Preview 链接短时有效，并绑定到具体 preview session。
- Provider 凭证保存在本地 settings/profile storage。
- Agent CLI 仍然在你的电脑上运行，权限也来自你的本地环境。

## 本地开发

```bash
cd src
bun install
bun run prebuild
bun run dev
```

环境要求：Rust 1.82+、Bun 1.3+，推荐 Node.js 24 LTS。macOS 需要 Xcode Command Line Tools；Linux 需要安装发行版对应的 WebKitGTK/Tauri 依赖。

## 文档

- [安装指南](https://github.com/jazzenchen/VibeAround/wiki/Setup-Guide-CN)
- [Launch、Profiles 与 Models](https://github.com/jazzenchen/VibeAround/wiki/Model-Profiles-and-Agent-Launch-CN)
- [支持的 Agent](https://github.com/jazzenchen/VibeAround/wiki/Supported-Agents-CN)
- [频道插件](https://github.com/jazzenchen/VibeAround/wiki/Channel-Plugins-CN)
- [配置模型](https://github.com/jazzenchen/VibeAround/wiki/Configuration-Model-CN)
- [Tunnels 与 Previews](https://github.com/jazzenchen/VibeAround/wiki/Tunnel-Configuration-CN)
- [架构说明](https://github.com/jazzenchen/VibeAround/wiki/Architecture-CN)
- [构建与打包](https://github.com/jazzenchen/VibeAround/wiki/Build-and-Packaging-CN)
- [FAQ 与故障排除](https://github.com/jazzenchen/VibeAround/wiki/FAQ-and-Troubleshooting-CN)

## 社区

提问、交流工作流，或者告诉我们哪个 Agent、Provider、Channel 还需要更顺手。

[![Discord](https://img.shields.io/badge/Discord-Join%20VibeAround-5865F2?logo=discord&logoColor=white)](https://discord.gg/KsJWkY64GN)
[![Product Hunt](https://img.shields.io/badge/Product%20Hunt-Follow%20VibeAround-DA552F?logo=producthunt&logoColor=white)](https://www.producthunt.com/products/vibearound)

中文交流微信群：

<img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/community/wechat-group-qr.webp" width="180" alt="VibeAround 微信群二维码" />

微信群二维码会周期性过期。如果图片失效，可以通过 Discord 或 GitHub Issues 索取最新二维码。

## 许可证

[MIT](LICENSE)
