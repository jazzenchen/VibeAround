# 核心概念

VibeAround 的运行时围绕六个概念构建：**Workspace（工作区）**、**Thread（会话线程）**、**Route（路由）**、**Session（会话）**、**Agent** 和 **Profile**。每个功能 —— IM 聊天、Web Chat、交接、Agent 切换 —— 都是这六者的组合。本页定义每一个概念以及它们的关系。

## 一张图看懂模型

```text
Route (telegram:chat_42) ──附着于──► Thread ──属于──► Workspace (~/dev/my-app)
Route (web:ws_wt_9f3e)   ──附着于──►   │
                                       │ 托管
                                       ▼
                               Agent 进程 (claude)
                                  │ 以……启动
                                  ▼
                               Profile (moonshot)
                                  │ 跟踪
                                  ▼
                               Session（CLI 原生 session id）
```

## Workspace

Workspace 是你机器上供 Agent 工作的目录 —— 通常是一个项目检出。Workspace 显式注册（桌面 UI、`va workspace add`，或在 Thread 需要时自动注册），各自获得稳定 id。对于从未选过项目的对话有两个默认值：Web 聊天回落到 **General** Workspace，每个 IM 渠道首次联系时得到 `<default_workspace>/im/<channel>` —— 所以新的 Telegram 聊天会先在 `…/im/telegram` 里工作，直到你 `/workspace --switch` 到真正的项目。

- 一个 Workspace 包含任意数量的 Thread。
- 删除或切换 Workspace 永远不碰目录内容；VibeAround 只管理自己的记录。

*细节：[workspace 模块内幕](../../internals/modules/workspace.md) · [`va workspace` 命令](../reference/cli.md#workspaces-previews-profiles) · [`default_workspace` / `workspaces` 设置](../reference/configuration.md#settingsjson)*

## Thread

Thread 是一段有连续性的对话：拥有"我们聊到哪了"的单位。每个 Thread 恰好属于一个 Workspace，记录着哪个 Agent 托管它、它产生过哪些 CLI Session、以及开启/关闭状态。Thread 状态以事件日志持久化，所以关闭的 Thread 仍可查看，开启的 Thread 能挺过守护进程重启。

- 聊天里 `/new` 关闭当前 Thread 并开一个新的。
- `/close` 只关闭，不开新的。
- Thread 可以托管**子 Agent** —— 为多 Agent 回合额外拉起的 Agent 进程 —— 与宿主 Agent 并存。

*细节：[会话生命周期](session-lifecycle.md)（开启/关闭规则、重启行为） · [workspace 模块内幕](../../internals/modules/workspace.md) · [子 Agent MCP 工具](../reference/api-surfaces.md#mcp-tools)*

## Route

Route 是穿过某渠道的一条对话路径的稳定地址：三元组 *(channel kind, bot id, chat id)* —— 例如 `telegram : mybot : chat_42` 或 `web : ws_wt_9f3e`。入站消息靠 Route 找到自己的 Thread：任一时刻，一条 Route 至多附着在一个开启的 Thread 上。

- 多条 Route 可以附着到同一个 Thread。会话交接就是这么回事：第二条 Route 附着到你在别处开始的 Thread 上。
- 同一 Route 上的消息严格按序处理；不同 Route 上的消息并行。

*细节：[IM 消息流](../../internals/flows/im-message.md)（Route 如何解析） · [channels 模块内幕](../../internals/modules/channels.md) · [交接流程](../../internals/flows/handover.md)*

## Agent

Agent 是 VibeAround 能驱动的编程 CLI：Claude Code、Codex、Gemini CLI、Cursor、Qwen Code、Kiro、OpenCode 或 Pi。VibeAround 通过 [Agent Client Protocol](https://agentclientprotocol.com)（ACP），经由各 Agent 的 ACP 适配器与之通信，每个活跃 Thread 拉起一个 Agent 进程。桌面版条目（`claude-desktop`、`codex-desktop`）只是启动目标，不是 ACP 运行时 —— 它们打开厂商的桌面应用。

- `/switch host <agent>` 切到不同 Agent 会开一个带全新 Session 的**新 Thread**（上下文不在不同 Agent 产品之间传递）；只切 Profile 则保留 Thread 和 Session。
- Agent 定义（ACP 适配器包、PTY 命令、恢复模板、配置注入路径）来自内置注册表。

*细节：[支持的 Agent 矩阵](../product/supported-matrix.md#编程-agent) · [agent 模块内幕](../../internals/modules/agent.md) · [启动子系统](../../internals/launch.md)（Agent 如何被启动） · [`/switch` 命令参考](../guides/im-usage.md#agent-与-profile)*

## Profile

模型 Profile 是一份保存好的供应商配置：哪个 API 端点、哪份凭据、哪些模型，以及内置 Bridge 应如何在 Agent 的原生 API 方言和供应商方言之间转换。Profile 让一份供应商订阅服务多个不同的 Agent CLI。

- Profile 按每次启动或每个 Thread 的宿主绑定来选择；同一 Thread 可以换一个 Profile 重新托管。
- 特殊的 `direct` Profile 意思是"让 Agent 用自己的官方登录启动，不经过 Bridge"。

*细节：[模型 Profile 指南](../guides/model-profiles.md)（配置方法） · [供应商端点参考](../reference/provider-endpoints.md)（套餐、base URL、模型） · [Bridge 机制](local-api-and-bridge.md) · [profiles 模块内幕](../../internals/modules/profiles.md)*

## Session

Session 是 Agent CLI 自己的对话记录 —— `claude --resume <id>` 或 `codex resume` 恢复的那个东西。VibeAround 在 Agent 创建 Session 时观察其 id 并存到 Thread 上，这正是跨界面连续性的原理：在 IM 里接续终端会话（`/pickup <code>`）、从 Web 控制台恢复原生会话、或把 Web 会话交接到手机。

- Session 属于 Agent，不属于 VibeAround。VibeAround 跟踪并重新发现它们（包括在 VibeAround 之外创建的），但对话记录存在 Agent 自己的存储里。

*细节：[会话生命周期](session-lifecycle.md)（什么能挺过什么） · [交接流程](../../internals/flows/handover.md) · [`va launch sessions` 命令](../reference/cli.md#agents-and-launches)*

## 各部分如何协作

一条消息到达某 Route。Route 解析到它附着的开启 Thread（首次联系时在默认 Workspace 创建 Thread）。Thread 确保宿主 Agent 进程在运行 —— 在 Workspace 目录里启动、绑定 Profile 的凭据已就位 —— 并确保 Agent Session 存在。提示通过 ACP 转发；输出沿着附着在该 Thread 上的每条 Route 流回。闲置十分钟后 Agent 进程被关停以节省资源；Thread 保持开启，下一条消息会透明地重新拉起 Agent 并恢复 Session。

---

*Source anchors: `src/core/src/routing.rs` (RouteKey), `src/core/src/workspace/` (workspaces, threads, attachments), `src/core/src/resources.rs` + `src/resources/agents.json` (agent registry), `src/core/src/profiles/` (profiles), `src/core/src/workspace/manager.rs` (AGENT_HOST_IDLE_SHUTDOWN_DELAY).*
*Last verified: v0.7.11*

<sub>[◀ 故障排查与 FAQ](../guides/troubleshooting-and-faq.md) · [文档索引](../README.md) · [工作原理 ▶](overview.md)</sub>
