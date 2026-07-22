# VibeAround 是什么

VibeAround 把安装在你机器上的 AI 编程 Agent 变成随处可达的东西：手机上的 IM 聊天、一个浏览器标签页、桌面控制应用、或另一台电脑上的终端。Agent、代码和凭据永远不离开你的机器 —— VibeAround 只是为它们加上了"门"。

## 它解决什么问题

Claude Code、Codex 这样的编程 Agent 是终端程序。坐在键盘前时它们很好用，人一走开就完全够不着。而真实的工作不会因为你离开桌子就停下：

- 你离开五分钟后，Agent 请求运行某条命令的权限。
- 一次长时间重构跑完了，你想在手机上审阅并回复。
- 你在终端里开始调试，想窝在沙发上从聊天窗口继续。
- 你想把正在运行的 dev server 分享给同事看一分钟，而不是部署它。

VibeAround 在本地运行一个守护进程（daemon），托管你的 Agent，并通过你已经在用的每一种界面把它们暴露出来，底层是同一套一致的对话模型。

## 你能用它做什么

**在任意 IM 里和本地 Agent 对话。** 连接 Telegram、Slack、飞书、Discord、微信、钉钉、企业微信或 QQ 机器人。完整的编程流程都能在聊天里完成：Agent 写代码、跑命令、用可点按的按钮请求权限、把结果流式发回来 —— 群聊和私聊都可以。

**一个对话跨多个界面延续。** 在终端里开始，运行 `/handover`，然后在任意已连接的 IM 里输入 `/pickup <code>`，带着完整上下文继续。Web 聊天同样可以在手机上接续。连续性之所以成立，是因为 VibeAround 跟踪的是 Agent 自己的 session id，而不是聊天记录。

**一份模型订阅供多个 CLI 使用。** 模型 Profile 加上内置的 API Bridge，让单个供应商账号（Moonshot/Kimi、DeepSeek、OpenRouter、MiniMax、Z.AI/GLM、Gemini、Azure OpenAI、xAI 等）驱动 Claude Code、Codex、Gemini CLI 以及任何 OpenAI 兼容客户端 —— 包括不同 API 方言之间的协议转换。

**在浏览器里操作一切。** Web 控制台提供真正的终端（xterm over WebSocket）、带权限卡片的 Web Chat、dev server 实时预览和 Markdown 渲染预览 —— 默认只在本机，也可以通过内置隧道（ngrok、localtunnel、Cloudflare 或 Tailscale Funnel）从任何地方访问，由配对码保护。

**按你的方式启动 Agent。** 桌面应用和 `va launch` 用保存好的 Profile 在你自己的终端里打开 Agent CLI：凭据就位、模型路由配置好、项目集成装好。

**同时运行多个 Agent。** Claude 在 Telegram、Codex 在 Slack 同时进行，各自拥有独立的会话线程（Thread）和工作区（Workspace）。Agent 还能在一个 Thread 内派生并行子 Agent，完成多 Agent 回合。

## 它不是什么

- **不是云服务。** 不存在 VibeAround 的服务端；守护进程、Agent、凭据和代码都留在你的机器上。隧道是自愿开启的，且有配对保护。
- **不是又一个编程 Agent。** VibeAround 不与 Claude Code 或 Codex 竞争；它通过开放的 Agent Client Protocol 托管并路由你已经拥有的 Agent。
- **不是模型 API 的聊天代理。** 对话经过真正的 Agent CLI，带着它们自己的工具、上下文管理和会话存储 —— 和你手动运行的是同一个 Agent，只是能从更多地方够到它。

## 系统的形状

一个 Rust 守护进程（`vibearound-server`）掌管运行时：渠道插件、Workspace 线程、Agent 进程、API Bridge、预览和隧道。桌面应用（Tauri）内嵌这个守护进程并提供 GUI 管理。Web 控制台、IM 插件、TUI 和 `va` CLI 都是同一个守护进程的客户端 —— 这就是为什么一个对话可以在它们之间移动而不丢失进度。

接下来读[核心概念](../architecture/concepts.md)，了解全部文档共用的六个词汇；或读[工作原理](../architecture/overview.md)看消息流转。

---

*Source anchors: `README.md` (positioning), `src/server/src/lib.rs` (daemon composition), `src/resources/agents.json` (agent registry), `src/resources/profile-catalog/` (providers).*
*Last verified: v0.7.11*

<sub>[文档索引](../README.md) · [支持矩阵 ▶](supported-matrix.md)</sub>
