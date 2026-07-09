# 会话生命周期

本页回答运维层面的问题：对话何时开始何时结束，重启时发生什么，交接会话或切换 Agent 时到底移动了什么。词汇定义见[核心概念](concepts.md)。

## Thread 生命周期

Thread 在某条 Route 第一次需要它时诞生 —— 聊天的第一条消息，或显式的 `/new` —— 然后保持**开启**，直到某件事关闭它：

| 事件 | 效果 |
|---|---|
| `/new` | 关闭当前 Thread，在同一 Workspace 新建一个，重新附着 Route |
| `/close` | 关闭 Thread；下一条消息会创建新的 |
| 不可恢复的 Agent 错误（如需要登录） | Thread 自动关闭，原因发到聊天里 |
| 守护进程关停 | Thread 保持开启 —— Thread 状态是磁盘上的事件日志 |

关闭的 Thread 在事件日志里保留历史；永远不会被悄悄删除。

## Thread 内的 Agent 进程生命周期

托管 Thread 的 Agent 进程刻意比 Thread 本身更短命：

```text
首条提示 ──► 拉起 Agent ──► 创建/恢复 CLI Session ──► 回合 ──► 闲置
                                                                │ 10 分钟
      下一条提示 ◄── 重新拉起 + 恢复 Session ◄── Agent 被关停 ◄──┘
```

- **闲时关停：** 最后一次活动十分钟后，宿主 Agent 进程被停止。聊天里对此无感 —— Thread 仍开启，CLI session id 被保留。
- **透明恢复：** 下一条提示重新拉起 Agent 并恢复记录的 CLI Session，上下文跨过这段空档。
- **崩溃：** Agent 进程在回合中不会被自动重启（重启策略是刻意的：崩溃以错误形式呈现，而不是静默重试）。下一条提示会启动新进程并恢复 Session。

## 守护进程重启后什么会保留

| 东西 | 保留？ | 说明 |
|---|---|---|
| 开启的 Thread 及其 Route 附着 | 保留 | 启动时从事件日志重建 |
| 每个 Thread 观察到的 CLI session id | 保留 | 存在 Thread 事件里 |
| Session 内的对话上下文 | 保留 | 归 Agent CLI 自己的存储所有；经恢复还原 |
| 进行中的回合 | 不保留 | 被重启打断的回合丢失；Session 从最后完成的状态恢复 |
| 浏览器里 Web Chat 的滚动历史 | 部分保留 | 启动回放会给 web Route 重发近期输出 |

## 交接：在界面之间移动对话

交接是把第二条 Route 附着到已有 Thread 上，或把外部 CLI Session 重新绑定进一个 Thread：

1. **终端 → IM。** 在启动的 Agent CLI 里，VibeAround 的 MCP 工具 `prepare_handover` 发出一个短寿命码。在任意已连接的 IM 里输入 `/pickup <code>`，那个聊天的 Route 就附着到绑定同一 Agent、Workspace 和 CLI Session 的 Thread 上 —— Agent 带完整上下文恢复。
2. **Web → 手机。** 控制台的交接流程走同一机制：Web Thread 的 Session 被某条 IM Route 接续。
3. **多个收听者。** 因为附着是叠加式的，输出会扇出到每条已附着的 Route：你可以在 Web 控制台和 Telegram 里同时看同一个回合。

接续码一次性、快速过期、**只存在于内存** —— 守护进程重启会清空它们，中间如果 VibeAround 重启过就重新发起交接。无效或已用过的码会以聊天消息报错，不会附着任何东西。

## 切换宿主 Agent

`/switch host <agent>`（或 `/switch <agent>`，可选 `<agent>+<profile>`）的行为取决于改变的是什么：

- **不同 Agent** → 创建**新 Thread**，带目标宿主和全新 CLI Session；旧 Thread 保持开启但失去 Route。对话上下文不跨 Agent 产品传递。
- **同一 Agent、不同 Profile** → 保留当前 Thread，**同一 Session 得以保持**；Agent 宿主在新 Profile 下重启并从原处恢复。
- 想回到之前某个 Agent 的对话，用 `/session` + `/session --switch <id>` —— Session 记录留在各自 Thread 上，即使 Route 已经移走。

## 多 Agent 回合与子 Agent

Thread 可以运行多 Agent 回合：宿主 Agent 用 `initialize_subagents` / `wait_for_subagents` MCP 工具在同一 Workspace 里拉起具名子 Agent（并行、协作或头脑风暴模式）。每个子 Agent 是拥有自己 CLI Session 的完整 Agent 进程，在 Thread 上被跟踪，完成报告收回到宿主的回合里。被打断的子 Agent 会在 Thread 运行时重建时恢复。

## 计时参考

所有生命周期计时器（闲时关停、心跳/看门狗、码的 TTL、分享链接过期）集中在一张权威表：[计时器与上限](../reference/timers-and-limits.md)。

---

*Source anchors: `src/core/src/workspace/threads/runtime.rs` (agent lifecycle, busy/failed), `src/core/src/workspace/manager.rs` (AGENT_HOST_IDLE_SHUTDOWN_DELAY, attachments), `src/core/src/channels/prompt/` (commands, auto-close), `src/core/src/workspace/handover.rs` (in-memory pickup codes), `src/core/src/channels/prompt/handler.rs` (switch_host: new-thread vs preserve-session split), `src/server/src/web_server/mcp/mod.rs` (subagent tools), `src/core/src/process/supervisor.rs` (tick, watchdog).*
*Last verified: v0.7.11*

<sub>[◀ 工作原理](overview.md) · [文档索引](../README.md) · [渠道插件系统 ▶](channel-plugin-system.md)</sub>
