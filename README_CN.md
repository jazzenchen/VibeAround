<div align="center">

<img src="Logo.svg" alt="VibeAround logo" width="96" />

# VibeAround

**AI 编程 Agent 的一站式 Hub。**

[下载](https://github.com/jazzenchen/VibeAround/releases/latest) | [演示](https://youtu.be/6kxNKTMz-AM) | [Wiki](https://github.com/jazzenchen/VibeAround/wiki) | [Discord](https://discord.gg/KsJWkY64GN) | [微信群](#社区) | [English](README.md)

</div>

<p align="center">
  <img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/documents/v0.7.7/readme/launch.webp" alt="VibeAround Launch 界面，可选择 Agent Profile 和 Workspace" width="92%" />
</p>

## 为什么需要 VibeAround

VibeAround 把分散的 AI 编程工作流收拢到一个入口，同时尽量不打扰你已经配置好的环境。

- 继续使用你已经熟悉的 Claude Code、Codex CLI、Gemini CLI、Pi、OpenCode、Claude Desktop、Codex Desktop 等 AI 编程 Agent。
- 直接启动 Agent，或通过第三方 AI API 运行 Agent，不用在不同 Agent 配置文件之间来回手改。
- 桥接不同 AI API 协议，让 Agent 和模型 provider 即使原生 API 不匹配也能配合工作。
- 在桌面、CLI、消息应用、手机浏览器、网页浏览器和 Web Terminal 之间继续同一个会话。
- 远程预览 Agent 生成的网站服务、Markdown 和 HTML，同时执行环境仍留在你的电脑上。
- 当所选模型 provider 不提供原生搜索时，为 Agent 补上 Web Search 等 host-side 工具能力。
- 在现有配置、项目权限和工作流之外增加能力，尽量保持原有环境干净、少改动。

## Agent Launch

选择合适的 AI Agent，搭配对应的模型，一键启动。

你可以自由组合 AI Agent、模型配置或 API 接口、工作目录，VibeAround 会帮你启动 Claude Code、Codex CLI、Gemini CLI、Pi、OpenCode、Claude Desktop、Codex Desktop 等工具，同时不改变原有的配置（如 SKILL，MCP 等）。

- 从同一个桌面 UI 启动 Claude、Codex 等 AI Agent 和桌面版 Agent。
- 自由选择 AI Agent、模型配置、API 接口和工作目录等。
- 开始全新会话，或者继续上次中断的工作。
- 支持直连启动，也支持基于 Profile 的灵活配置，包括 Claude Desktop 和 Codex Desktop 的 Profile overlay。
- 记录并检查本次 launch/session 的 API 流量，包括 original request、bridge request、raw response、bridge response 和 search tool 内容。
- 不改变每个 AI Agent 自己的配置文件、工作流和项目上下文。
- VibeAround 不会修改原始 CLI 配置文件。如果你使用 CC Switch 等工具，建议手动删除可能冲突的 Profile 配置项，例如 `~/.claude/settings.json` 里的 `env`。

<p align="center">
  <img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/documents/v0.7.7/readme/api-inspector.webp" alt="VibeAround Bridge recorder，可查看请求、响应和搜索细节" width="88%" />
</p>

### VibeAround vs CC Switch

| 维度 | VibeAround | [CC Switch](https://github.com/farion1231/cc-switch) |
|---|---|---|
| Agents | ✅ Claude Code、Codex CLI、Gemini CLI、Pi、OpenCode、Claude Desktop、Codex Desktop、Cursor CLI、Kiro CLI、Qwen Code、Trae（coming soon） | ✅ Claude Code、Claude Desktop、Codex、Gemini CLI、OpenCode、OpenClaw 和 Hermes |
| Agent launch | ✅ CLI 和桌面版 Agent 都可一键启动，并在启动前选择 Profile、API endpoint、Workspace、Terminal 和 Session | ⚠️ 仅支持 macOS launch |
| Run multiple providers at once | ✅ 同一个 Agent 可在不同 session 中使用不同 API Profile | ❌ 必须切换 Profile / active config |
| API Bridge | ✅ OpenAI Responses、Chat Completions、Anthropic Messages 和 Gemini Generate Content | ✅ OpenAI Responses、Chat Completions、Anthropic Messages 和 Gemini Generate Content |
| Live request inspect | ✅ Original request、bridge request、raw response、bridge response 和 search tool 内容 | ❌ 当前不支持 |
| Session resume | ✅ 在 macOS、Windows 和 Linux 上继续并启动 CLI 与桌面版 Agent | ⚠️ macOS 支持 terminal resume；Windows 和 Linux 只能复制 command 到剪贴板 |
| Workspace selection | ✅ 从指定目录 launch agent | ⚠️ 仅支持 OpenClaw workspace |
| IM Chat | ✅ 通过 [Remote Messaging & Session Continuity](#remote-messaging--session-continuity) 接入飞书/Lark、Discord、Slack 等 | ❌ 当前不支持 |
| Web Terminal | ✅ 通过 [Web Terminal](#web-terminal) 远程控制 CLI | ❌ 当前不支持 |
| Web Hub | ✅ 通过 [Web Hub](#web-hub) 在电脑或手机浏览器 launch、continue sessions 和 chat | ❌ 当前不支持 |
| Remote preview | ✅ 通过 [Live Preview](#live-preview) 预览 dev server / Markdown / HTML 链接 | ❌ 当前不支持 |
| Host-side web search | ✅ provider 不提供原生搜索时，通过 [Host-side Web Search](#host-side-web-search) / `va-search-tool` 补上 | ❌ 当前不支持 |
| MCP 和 Skills | ❌ 当前不支持 | ✅ 在 supported apps 之间统一管理 MCP 和 Skills |
| Usage / cost tracking | 🚧 Roadmap | ✅ 内置 usage dashboard |

## Known Issue

### 遇到 `Unable to connect to API (ConnectionRefused)` 怎么办？

这通常表示 Agent 仍在读取 CC Switch 或类似配置切换工具写入的 API 设置。这些设置可能指向一个已经没有运行的本地 proxy，所以 Agent 在 VibeAround 接管路由之前就连接失败了。

请到对应 Agent 的配置文件里删除冲突字段，然后再从 VibeAround 启动。以 Claude Code 为例，可以检查 `~/.claude/settings.json`，删除旧的 provider 配置，例如设置 base URL、API key 或 proxy endpoint 的 `env` block。如果项目目录里还有 Claude 的 project-level 配置文件，也一起检查。

## API Profiles & Bridge

通过在 Profile 中保存第三方 AI API 配置，使用 VibeAround 的本地 API 中转能力，可以实现在常见 AI API 之间自由转换，从而打破 API 无法在特定 AI Agent 中使用的问题。

```text
Agent 侧 API 形态                  VibeAround API Bridge                  Provider 侧 API 形态
+-------------------------+        +-----------------------------+        +-------------------------+
| OpenAI Responses        | ----\  | 按 Profile 暴露本地路由     |   /->  | OpenAI Responses        |
| OpenAI Chat Completions | -----\ | 模型别名与 metadata         |  /-->  | OpenAI Chat Completions |
| Anthropic Messages      | -----> | 请求 / 响应转译             |  --->  | Anthropic Messages      |
| Gemini Generate Content | -----/ | va-ai-api-bridge (VAAAB)    |  \\--> | Gemini Generate Content |
+-------------------------+        +-----------------------------+        +-------------------------+
```

*API 中转 能力由 [va-ai-api-bridge](https://github.com/jazzenchen/va-ai-api-bridge) 项目提供。*

<p align="center">
  <img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/documents/v0.7.7/readme/api-bridge.webp" alt="VibeAround API Bridge 连接设置界面" width="88%" />
</p>

| API 形态 | 常见 endpoint |
|---|---|
| OpenAI Responses | `/v1/responses` |
| OpenAI Chat Completions | `/v1/chat/completions` |
| Anthropic Messages | `/v1/messages` |
| Gemini Generate Content | `/v1beta/models/{model}:generateContent` |

内置 provider preset 包括 DeepSeek、Alibaba DashScope、Moonshot / Kimi、MiniMax、Xiaomi MiMo、xAI / Grok、NVIDIA NIM、Z.AI / GLM、Google Gemini、OpenRouter、Azure OpenAI 和自定义 endpoint。

- 保存 API 配置，包括 key、base URL、模型、alias 和 metadata。
- 在 OpenAI Responses、OpenAI Chat Completions、Anthropic Messages 和 Gemini Generate Content 等 API 形态之间自由转换。
- 支持同一个 AI Agent 在多个 session 里同时运行不同 API 配置。

## Host-side Web Search

即使当前模型 provider 不提供原生服务端搜索，VibeAround 也可以为 Agent 补上 Web Search 能力。

VibeAround 可以把 provider 原生的 `web_search` 替换成本机搜索运行时，再通过 bridge 把标准化后的搜索结果交回给模型。搜索源在 Settings 中配置，并通过 [va-search-tool](https://github.com/jazzenchen/va-search-tool) 项目运行。同一套 search SDK / runtime 也可以脱离 VibeAround，以命令行方式单独运行，用于本地 smoke test 或自定义集成。

### va-search-tool

[va-search-tool](https://github.com/jazzenchen/va-search-tool) 是 VibeAround host-side web search 背后的独立搜索 runtime。它既可以作为 VibeAround 托管的 plugin 通过 stdio 运行，也可以作为一次性搜索的 CLI 使用，还可以启动一个提供 `/v1/search` 的本地 HTTP 服务。目前支持 Exa、Tavily、Grok / xAI 和 Brave Search。

- 支持 Exa、Tavily、Grok / xAI 和 Brave Search 作为 host-side 搜索源。
- 可以用 [`va-search-tool`](https://github.com/jazzenchen/va-search-tool) `search ...` 独立跑 CLI 检查，也可以暴露本地 `/v1/search` 服务，把搜索 runtime 用在 VibeAround 之外。
- 可以在 Web Search 设置页保存前直接测试当前搜索配置。
- Bridge recorder 中可以查看搜索请求，并按 source 区分搜索结果。
- API key 保存在本地设置中，AI provider 只会收到标准化后的搜索结果。

## Agent as API

把本地 AI 编程 Agent 当成 API endpoint，用于开发和本地测试。

VibeAround 可以把已启用的本地 Agent 暴露成 OpenAI / Anthropic 兼容 API，让你在不部署生产网关的情况下，用 Claude Code、Codex CLI、Gemini CLI、OpenCode 等本地 Agent 测试应用集成。

<p align="center">
  <img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/documents/v0.7.7/readme/agent-as-api.webp" alt="VibeAround Local API Workbench，用本地 Agent 测试 API 请求" width="88%" />
</p>

| API 形态 | Local agent endpoint |
|---|---|
| OpenAI Responses | `/local-agent/{agent_id}/{profile_id}/v1/responses` |
| OpenAI Chat Completions | `/local-agent/{agent_id}/{profile_id}/v1/chat/completions` |
| Anthropic Messages | `/local-agent/{agent_id}/{profile_id}/v1/messages` |
| Models | `/local-agent/{agent_id}/{profile_id}/v1/models` |

- 先在 Local API Workbench 中测试本地 Agent，再接入你的应用。
- 在 VibeAround 中选择 Agent 和 Profile，然后调用对应的本地 endpoint。
- 通过 `x-vibearound-cwd` 指定请求运行的 workspace directory。
- 支持这些 API 形态下的 streaming / non-streaming 请求。
- 这个能力面向 dev 和 local test usage；VibeAround 不试图成为托管式生产 API gateway。

## Remote Messaging & Session Continuity

把正在跑的 coding session 交接出去，再从任意地方接回来。

VibeAround 可以为当前桌面或 CLI 会话生成一个 `/pickup` command。把这个 command 发到任意已连接的 IM channel，就能用同一个 Agent 继续同一个 session；pickup 不绑定生成它的那个 channel。

IM 接入是通过 [VibeAround Channel SDK](https://github.com/jazzenchen/va-plugin-channel-sdk) + plugin system 实现的。每个消息平台都可以作为托管插件接入同一套 session 和 command model，而不是硬编码进核心应用。

### VibeAround Channel SDK

[VibeAround Channel SDK](https://github.com/jazzenchen/va-plugin-channel-sdk) 是用来构建 IM channel plugin 的 SDK。它负责 VibeAround agent/session 生命周期和 command bridge，各个 plugin 只需要专注于具体平台的消息传输和展示能力，比如飞书/Lark、Telegram、Slack、Discord、钉钉、企业微信或 QQ Bot。

<p align="center">
  <img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/documents/v0.7.7/readme/im-remote.webp" alt="VibeAround Remote Access 设置，包含消息应用和 Cloudflare tunnel" width="88%" />
</p>

- 从桌面、CLI 或 Web Hub hand over 当前 session。
- 在飞书/Lark、Discord、Slack 或其他已配置消息频道中 pickup。
- 通过基于 SDK 的 plugin 添加或更新消息频道。
- 远程继续同一个 Agent session，不丢失上下文。
- pickup 之后可以直接在消息应用里和 AI Agent 对话。
- 用 `/switch` 切换 Workspace 和 Agent。
- 在同一个对话里打开预览链接。

### VibeAround vs cc-connect

| 维度 | VibeAround | [cc-connect](https://github.com/chenhg5/cc-connect) |
|---|---|---|
| Handover / pickup | ✅ 从当前 session 生成 `/pickup` command，并在任意已连接 IM channel 中继续 | ❌ 当前不支持 |
| 聊天平台 | ✅ 飞书/Lark、Discord、Slack、Telegram、微信、钉钉、企业微信、QQ Bot | ✅ 飞书/Lark、WPS 协作、钉钉、Telegram、Slack、Discord、LINE、企业微信、微博、个人微信、QQ、QQ Bot、Matrix |
| Agent 连接 | ✅ 配合 VibeAround 启动的 Agent、Profile 和 Workspace 使用 | ✅ 支持 10+ agents 和 ACP-compatible agents |
| Session commands | ✅ `/session --list`、`/session --switch`、`/new` 和 `/pickup` | ✅ `/new`、`/list`、`/switch` 和 `/current` |
| Agent / profile switch | ✅ `/switch`、`/agent --switch` 和 `/profile --switch` | ❌ 当前不支持 |
| Workspace commands | ✅ `/workspace --list` 和 `/workspace --switch` | ✅ `/dir` 和 `/cd` |
| Remote preview | ✅ 发送 dev server、Markdown、HTML 的 Live Preview 链接 | ❌ 当前不支持 |
| IM file attachments | ⚠️ 只支持发送；暂不支持从 IM 接收文件 | ✅ 在支持的平台上可发送和接收 files/images |
| Web Terminal | ✅ 用浏览器远程控制本地 AI Agent CLI | ❌ 当前不支持 |
| Web Hub | ✅ 从浏览器 launch、continue sessions 和 chat | ⚠️ 提供 Web admin/config dashboard；service 需要单独运行 |
| Scheduling 和 rich IM commands | ❌ 当前不支持 | ✅ `/timer`、`/cron`、`/cancel`、`/ps`、provider/model/mode commands |

## Web Terminal

用电脑或手机浏览器直接控制本地 AI Agent CLI。

VibeAround 通过 Web Dashboard 暴露本地 terminal session，让你可以从电脑或手机浏览器远程操作 AI Agent CLI，同时执行环境仍留在自己的电脑上。

<p align="center">
  <img src="assets/marketing/screenshots/en/web-terminal.png" alt="VibeAround Web Terminal 远程控制界面" width="88%" />
</p>

- 通过电脑或手机浏览器里的 Web Terminal 访问本地 AI Agent CLI。
- 使用 tunnel 远程访问，同时 daemon 仍在本地运行。
- 保留本机项目权限和 terminal 环境。

## Web Hub

从电脑或手机浏览器启动并继续会话。

VibeAround Web Hub 提供浏览器入口，用来选择 Agent、API Profile、Workspace 和 Session，并在不同设备之间继续同一项工作。

<table>
  <tr>
    <td width="72%" align="center"><img src="assets/marketing/screenshots/en/cover-web-dashboard.png" alt="VibeAround 中文 Web Dashboard 封面" width="92%" /></td>
    <td width="28%" align="center"><img src="assets/marketing/screenshots/en/web-dashboard-mobile.png" alt="VibeAround mobile web dashboard" width="220" /></td>
  </tr>
</table>

- 从浏览器开始新会话，或继续已有会话。
- 在浏览器里选择 Agent、API Profile、Workspace 和 Session。
- 使用电脑或手机浏览器访问，不需要把执行环境搬到云端。

## Remote Tunnels

只在你明确开启时，才把 VibeAround 的本地 Web 入口暴露出去。

Remote tunnel 会被 Web Hub、Web Terminal、Live Preview 和 Markdown preview 链接使用。VibeAround 仍然让 daemon 留在本地运行，只负责启动你选择的 tunnel provider，并且公网 tunnel URL 需要浏览器配对后才能访问。

| Tunnel 选项 | 状态 | 说明 |
|---|---|---|
| Local only | ✅ 支持 | 不开启公网 tunnel；daemon 只监听 loopback。 |
| Localtunnel | ✅ 支持 | 通过托管的 `localtunnel` npm package 或系统 `npx` 快速生成公网 URL。 |
| Cloudflare Tunnel | ✅ 支持 | 通过 `cloudflared`、tunnel token 和配置好的 hostname 使用 named tunnel。 |
| ngrok | ✅ 支持 | 使用 ngrok SDK，支持 auth token 和可选 reserved/static domain。 |
| Tailscale Funnel | 🚧 Roadmap | 面向已经用 Tailscale 连接多台设备的用户，计划支持。 |

## Live Preview

预览 AI Agent 执行任务生成的结果。

VibeAround 可以把网站服务、Markdown 文件、HTML 文件等生成产物变成可打开的预览链接。你可以从电脑或手机浏览器、消息应用里直接查看结果。

<p align="center">
  <img src="assets/marketing/screenshots/zh-CN/preview-in-a-row.webp" alt="从消息应用发起预览、配对浏览器、打开网页预览和 Markdown 预览" width="92%" />
</p>

- 生成可预览的链接和限时的分享链接。
- 通过 tunnel 可以实现远程访问预览链接。
- 可以预览网站服务、Markdown 文件、HTML 文件。

## 支持的 AI Agent

| Agent | 启动 | 继续 / 交接 | Profile 路由 |
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

✅ 支持 · ⚠️ 部分支持 · ➜ 直连启动 · — 暂不支持

## 支持的 Provider

内置 Profile 已覆盖主流官方 provider 和兼容 provider。只要你的 provider 支持相应的 API 形态，也可以通过自定义 endpoint 接入。

| Provider | 说明 |
|---|---|
| DeepSeek | 支持 OpenAI 兼容模式与 bridge 路由，可配置模型别名和 Claude 后缀归一化 |
| Alibaba DashScope | 支持 Coding Plan 与 Token Plan 两种 endpoint |
| Moonshot / Kimi | 支持 OpenAI 兼容模式与 Anthropic 风格 bridge flow |
| MiniMax | 支持 OpenAI 兼容模式与 Anthropic 风格 bridge flow |
| Xiaomi MiMo | 支持 Token Plan 与多区域 endpoint，并处理了该 provider 特有的返回格式 |
| xAI / Grok | 支持 Responses 和 Chat 两种 API 形态 |
| NVIDIA NIM | 支持 OpenAI 兼容的 Chat Completions |
| Z.AI / GLM | 内置 compatible endpoint |
| Google Gemini | 使用原生 Gemini API profile |
| OpenRouter | 提供 OpenAI 兼容 profile |
| Azure OpenAI | 支持 Responses 和 Chat deployment 路由 |
| 自定义 endpoint | 允许自定义 base URL、headers、模型列表和 API 形态 |

## 消息频道

消息频道通过 [VibeAround Channel SDK](https://github.com/jazzenchen/va-plugin-channel-sdk) 构建，再由 VibeAround 安装和统一管理。

| 频道 | 接入方式 | 适用场景 |
|---|---|---|
| Telegram | 通过 BotFather 创建 Bot 获取 Token | 个人机器人、移动端对话 |
| 飞书 / Lark | 使用飞书应用凭证（App ID / Secret） | 团队 IM、企业机器人 |
| Discord | 创建 Discord Bot 获取 Token | 服务器与私信工作流 |
| Slack | 配置 Bot / App Token 并开启 Socket Mode | 工作区私信工作流 |
| 微信 | 通过 OpenClaw 兼容 bridge 扫码登录 | 中文环境个人聊天 |
| 钉钉 | 使用 Stream API 凭证 | 企业聊天 |
| 企业微信 | 配置 WebSocket Bot 凭证 | 企业微信工作流 |
| QQ Bot | 使用 QQ 频道机器人凭证 | QQ 频道工作流 |

## Local-first 安全模型

VibeAround 默认把 AI 编程工作留在你自己的电脑上。

- Agent 在你的本地电脑上运行。
- Provider 密钥保存在 VibeAround 本地的设置和 Profile 存储中。
- Daemon 默认只监听 loopback，除非你显式开启 tunnel。
- Dashboard API 和 WebSocket 路由需要本地授权 token。
- 公网 tunnel URL 需要浏览器配对，不会直接暴露。
- Preview 链接有明确的作用域，并且短期有效。
- Agent CLI 使用你本机的项目权限，不越界。

---

## 快速开始

1. 下载适合你平台的最新桌面安装包。
2. 打开 VibeAround，跟随引导完成初始设置。
3. 启用你常用的 Agent CLI。
4. 如果希望 VibeAround 统一路由模型流量，添加 API Profile。
5. 在 Launch 中选择 Agent、模型 Profile、Terminal、Workspace 和 Session。
6. 之后，你就可以从桌面、Web Hub、Web Terminal 或配置好的消息频道继续工作。

详细文档见 [Wiki](https://github.com/jazzenchen/VibeAround/wiki)。

## 下载

最新版本：[VibeAround v0.7.9](https://github.com/jazzenchen/VibeAround/releases/tag/v0.7.9)。

| 平台 | 推荐下载 |
|---|---|
| macOS Apple Silicon | [VibeAround-macOS-arm64-0.7.9.dmg](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.9/VibeAround-macOS-arm64-0.7.9.dmg) |
| Windows x64 | [Setup EXE](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.9/VibeAround-Windows-x64-Setup-0.7.9.exe)、[MSI](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.9/VibeAround-Windows-x64-MSI-0.7.9.msi) 或 [免安装 ZIP](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.9/VibeAround-Windows-x64-Portable-0.7.9.zip) |
| Linux x64 | [AppImage](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.9/VibeAround-Linux-x64-AppImage-0.7.9.AppImage) 或 [deb](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.9/VibeAround-Linux-x64-DEB-0.7.9.deb) |

Windows 和 Linux 包由 GitHub Actions 构建。macOS 当前只提供 Apple Silicon 版本。

<a id="migration-guide-from-06x-cn"></a>

### 从 0.6.x 迁移指南

v0.7.3 调整了 Startkit 状态、Agent 来源检测、桌面启动目标和 Profile 启动设置。如果你从 0.6.x 升级，建议做一次干净的本地状态迁移：

1. 退出 VibeAround。
2. 完整备份旧的 `~/.vibearound` 目录。
3. 删除旧的 `~/.vibearound` 目录。
4. 只从备份里恢复持久状态。
5. 启动 VibeAround v0.7.3；如果 Launch、Profile、Startkit 或桌面版 Agent 设置看起来异常，再重新跑 onboarding / Startkit 配置。

只恢复这些持久状态：`settings.json`、`profiles/`、`google-oauth/`、`agents.json`、`launcher.json`、`state/`、`sessions/`、`launch-session-archive.json`、`workspaces/`、`worktrees/`。

不要恢复这些可重建的缓存/运行期数据：`.cache/`、`cache/startkit/`、`agents.detected.json`、`desktop-apps.detected.json`、`profile-state/`、`api-bridge/launches/`、`logs/`、`npm-global/`、`plugins/`、`bin/`、`runtime/`、`auth.json`。

macOS / Linux：

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

Windows PowerShell：

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

## 本地开发

```bash
cd src
bun install
bun run prebuild
bun run dev
```

环境要求：Rust 1.82+、Bun 1.3+，推荐 Node.js 24 LTS。macOS 需要 Xcode Command Line Tools；Linux 需要安装发行版对应的 WebKitGTK / Tauri 依赖。

## 文档

文档还在建设中，可能会稍微落后于快速迭代中的功能。

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

VibeAround 还处在快速打磨阶段，目前也主要是我一个人在开发。我的目标是做一个 all-in-one agent hub，让更多人可以更方便、更容易地拥抱 AI。现在它在测试覆盖和交互细节上都还有不够成熟的地方，也真诚期待 VibeAround 可以和社区一起被打磨出来。欢迎参与讨论、贡献代码和想法、分享工作流，或者提交 bug。

[![Discord](https://img.shields.io/badge/Discord-Join%20VibeAround-5865F2?logo=discord&logoColor=white)](https://discord.gg/KsJWkY64GN)
[![Product Hunt](https://img.shields.io/badge/Product%20Hunt-Follow%20VibeAround-DA552F?logo=producthunt&logoColor=white)](https://www.producthunt.com/products/vibearound)

友链社区：[LINUX DO](https://linux.do)

微信交流群：

<img src="assets/community/wechat-group-qr-2026-06-28.webp" width="180" alt="VibeAround 微信群二维码，有效期至 2026 年 6 月 28 日" />

该微信群二维码有效期至 2026 年 6 月 28 日。如果图片失效，可以通过 Discord 或 GitHub Issues 索取最新二维码。

## 许可证

[MIT](LICENSE)
