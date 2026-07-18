# Flow: Web Chat

Dashboard 的 Web Chat 里输入的一条消息如何到达 agent。后半段和 [IM 消息流程](im-message.md) 完全相同；本页只讲 web 边界不同的部分：socket 协议、session intents 和 replay。

## 连接建立

打开 Web Chat 会建立 `/va/ws/chat`（token-authenticated）。连接时 server 会：

1. 在 `WebChannelManager` 下以完整 route 注册连接（一个 thread 多个 tab = 多个 connection，都会收到同样的 fan-out），
2. 发送 `Config` event（enabled agents、default agent），
3. replay 该 route 最近的 output，让重新打开的 tab 能看到对话尾部。

→ `src/server/src/web_server/ws_chat.rs`, `src/core/src/channels/transport_websocket.rs`

Web channel 是**进程内**的：它不是 stdio plugin，而是在同一个 `PluginHost` 表里注册 `WebSocketPluginRuntime`。所有界面共用一套 outbound routing 机制。

## 入站消息形状

浏览器发送 typed JSON，而不是裸文本。主要类型：

| Type | 含义 |
|---|---|
| message（可带 `session_intent`、`profile`、`session_mode`） | 一条 prompt，可能附带 launch selection |
| `stop` | 取消当前 turn，并使同 route 上更早排队的 prompt 失效 |
| `PermissionResponse` | 点按后的权限卡片（[权限流程](permission.md)） |
| `SetMode` / `SetConfigOption` | 修改 agent session mode / config option |
| `ResumeSession` | 把一个原生 CLI session 附着到这个 web thread |

`stop` 作为优先控制操作进入 `ConversationIngress`：先提升 route lane 的 stop generation，再取消 runtime，因此覆盖“session 已建但 `agent.prompt` 尚未开始”的竞态窗口。

## Session-intent 步骤

这是 web 专属部分。Socket handler 在派发 prompt 前，会应用 message 携带的 launch selection：

- **`New { cwd }`**：创建 fresh thread，放到给定目录的 workspace（或当前 workspace）。
- **`Resume { agent, session_id, cwd }`**：把一个已有的原生 CLI session 绑定进 web thread（和 handover pickup 使用同一机制）。
- **none**：如果 agent/profile selection 有变化，就应用到 route 当前 thread。

随后 message 作为普通 `ChannelInput::Message` 进入所有 channel 共用的 `ConversationIngress`；从那里开始，[IM 消息流程](im-message.md) 第 4 到 10 步完全复用。

→ `ws_chat.rs` (`WebChatSessionIntent`, `apply_web_launch_selection`), then `src/core/src/channels/prompt/`

> Ordering note：intent side-effects 在 socket task 里执行，早于 queue 的 per-route serialization。单 tab 时看不出来；两个 tab 在同一个 thread 上竞速 launch selection 时可能交错。remediation plan 中作为已知 cleanup 跟踪。

## 出站：fan-out 与 Host 常驻

Web route 的 output 会派发给该完整 route 下所有已注册 connection；每个 output 变成 JSON `ChatEvent`（message chunks、tool status、permission cards、`TurnStatus`）。Inactive turn status 会在本 turn 的 notification outputs 之后发出，是公开的完成边界。

Web Chat 没有 route 专属的进程 idle deadline。`TurnStatus { active: false }`、socket 断开和关闭标签页都不会 unload Host，也不会关闭 Thread。它与 IM 共用 warm Thread 池策略：Host 保持常驻；只有以后真正的新 Host 让池超过软上限，且这个 Thread 是符合条件、最近最少活动的候选者时，才会被回收。回收保留 `ThreadRuntime` 与 Session；重新打开仍有 output replay，下一条提示需要时会恢复。

→ `ws_chat.rs` (`output_to_chat_event`), `transport_websocket.rs`（connection fan-out），`workspace/manager_routes.rs`（共用 warm Thread 池）

## TUI

TUI chat 作为自己的进程内 channel kind（`tui`）注册，使用同一套 WebSocket plugin runtime 机制和 `/va/ws/chat` contract。Web/TUI 都注册在 channel hub/PluginHost 路由边界，但不是 stdio plugin child，也不由 channel plugin supervisor 纳管。

2026-07-11 已对真实 server + Codex ACP 做 smoke：鉴权负例、同 route 双连接 fan-out、reconnect、`WS_ACP_OK` 流式返回和 session-ready 后立即 Stop 均通过；Stop 用例未泄漏 agent text chunk。

---

*Source anchors: `src/server/src/web_server/ws_chat.rs` (socket loop, intents, events), `src/core/src/channels/transport_websocket.rs` (WebChannelManager, fan-out/replay), `src/server/src/lib.rs` (web/tui channel registration, dispatch task), `src/core/src/workspace/manager_routes.rs` (shared warm-thread pool).*
*Last verified: `codex/im-acp-route-refactor` at `0ba7fa2e`（2026-07-11）。*

<sub>[◀ Flow: IM 消息](im-message.md) · [文档索引](../../README.md) · [Flow: 权限请求 ▶](permission.md)</sub>
