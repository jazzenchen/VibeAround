# Flow: Web Chat

Dashboard 的 Web Chat 里输入的一条消息如何到达 agent。后半段和 [IM 消息流程](im-message.md) 完全相同；本页只讲 web 边界不同的部分：socket 协议、session intents 和 replay。

## 连接建立

打开 Web Chat 会建立 `/ws/chat`（token-authenticated）。连接时 server 会：

1. 在 `WebChannelManager` 下以 route 的 chat id 注册连接（一个 thread 多个 tab = 多个 connection，都会收到同样的 fan-out），
2. 发送 `Config` event（enabled agents、default agent），
3. replay 该 route 最近的 output，让重新打开的 tab 能看到对话尾部。

→ `src/server/src/web_server/ws_chat.rs`, `src/core/src/channels/transport_websocket.rs`

Web channel 是**进程内**的：它不是 stdio plugin，而是在同一个 `PluginHost` 表里注册 `WebSocketPluginRuntime`。所有界面共用一套 outbound routing 机制。

## 入站消息形状

浏览器发送 typed JSON，而不是裸文本。主要类型：

| Type | 含义 |
|---|---|
| message（可带 `session_intent`、`profile`、`session_mode`） | 一条 prompt，可能附带 launch selection |
| `stop` | 取消正在执行的 turn † |
| `PermissionResponse` | 点按后的权限卡片（[权限流程](permission.md)） |
| `SetMode` / `SetConfigOption` | 修改 agent session mode / config option |
| `ResumeSession` | 把一个原生 CLI session 附着到这个 web thread |

> † 已知缺口：`stop` 目前和 message 走同一个 per-route FIFO queue，所以它只能在正在执行的 turn 结束后才处理。remediation plan 中以 H12 跟踪。

## Session-intent 步骤

这是 web 专属部分。Socket handler 在派发 prompt 前，会应用 message 携带的 launch selection：

- **`New { cwd }`**：创建 fresh thread，放到给定目录的 workspace（或当前 workspace）。
- **`Resume { agent, session_id, cwd }`**：把一个已有的原生 CLI session 绑定进 web thread（和 handover pickup 使用同一机制）。
- **none**：如果 agent/profile selection 有变化，就应用到 route 当前 thread。

随后 message 作为普通 `ChannelInput::Message` 进入所有 channel 共用的 sharded queue；从那里开始，[IM 消息流程](im-message.md) 第 4 到 10 步完全复用，包括同样的 command grammar、thread resolution 和 agent path。

→ `ws_chat.rs` (`WebChatSessionIntent`, `apply_web_launch_selection`), then `src/core/src/channels/prompt/`

> Ordering note：intent side-effects 在 socket task 里执行，早于 queue 的 per-route serialization。单 tab 时看不出来；两个 tab 在同一个 thread 上竞速 launch selection 时可能交错。remediation plan 中作为已知 cleanup 跟踪。

## 出站：fan-out 与 idle

Web route 的 output 会派发给该 chat id 下所有已注册 connection；每个 output 变成 JSON `ChatEvent`（message chunks、tool status、permission cards、`PromptDone`）。

Web thread 参与 idle 管理：活动会刷新 route 的 idle deadline；deadline 过期会 unload agent（thread 仍 open，replay + resume 让重新打开无缝）。关闭 tab 不会关闭 thread。

→ `ws_chat.rs` (`output_to_chat_event`), `transport_websocket.rs` (idle bookkeeping)

## TUI

TUI chat 作为自己的进程内 channel kind（`tui`）注册，使用同一套 WebSocket plugin runtime 机制和 `/ws/chat` contract。本页内容都适用于 TUI，除了浏览器专属的 replay UI。

---

*Source anchors: `src/server/src/web_server/ws_chat.rs` (socket loop, intents, events), `src/core/src/channels/transport_websocket.rs` (WebChannelManager, idle), `src/server/src/lib.rs` (web/tui channel registration, dispatch task).*
*Last verified: v0.7.11*

<sub>[◀ Flow: IM 消息](im-message.md) · [文档索引](../../README.md) · [Flow: 权限请求 ▶](permission.md)</sub>
