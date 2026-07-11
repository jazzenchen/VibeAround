# Flow: IM 消息

跟踪 Telegram/飞书/Slack 的一条私聊消息，从平台事件一直到流式回复。这是当前产品主干，[Web Chat](web-chat.md) 和 [权限请求](permission.md) 都从它分叉。群聊寻址逻辑仍保留在 adapter 中，但不属于当前 release 验收范围。文件引用均为仓库相对路径；行级细节在锚定文件的 module docs 里。

## 逐跳

```text
platform ─1─► plugin ─2─► stdio ─3─► input queue ─4─► bounded RouteLane
                                                          │5
                              ┌───────────────────────────┘
                              ▼
                     command? ──yes──► workspace-thread command handler
                        │no
                        ▼6                 ▼7                  ▼8
                  resolve route ──► ensure agent+session ──► ACP prompt
                                                              │
              chat ◄─10─ plugin ◄─ ChannelOutput ◄─9─ notifications
```

**1. Platform → plugin。** Channel plugin（独立 Node.js 进程）收到 webhook/long-poll event，应用平台语义，把附件下载到 `~/.vibearound/.cache/` 并构建 SDK prompt。DM 隐式寻址当前 bot。Dormant 群聊路径仍要求 @ 当前 bot，但群聊行为延后验收；Weixin 当前直接拒绝所有带 `group_id` 的 event，避免把群消息误判为 DM。逻辑 route 是 `(channel_kind, channel_instance_id, chat_id, actor_id?, topic_id?)`。
→ plugin repo；envelope type 在 `src/core/src/channels/types.rs`

**2. Plugin → daemon。** SDK 通过 stdio JSON-RPC/ACP 发送数据。`ChannelPluginRunner` 拥有一个 protocol generation；transport 把 `agent/prompt` 与 `va.channel` metadata 解码成 `ChannelInput`。官方插件均使用 `sendChannelPrompt` 并携带可获得的 sender/message/topic identity。Legacy 第三方插件仍可运行，但默认 `bot_id/actor_id` 不等于 multi-bot 支持。
→ `src/core/src/channels/plugin_runner.rs`, `transport_stdio/`, `types.rs`

**3. Enqueue。** `ChannelManager::handle_input` 是 fire-and-forget：input 先经过进程内 async handoff buffer，再进入 route dispatch。它只是实现层 mailbox，不是带持久化、重试或 replay 的业务消息队列；平台代码不会等待 agent 工作。
→ `src/core/src/channels/mod.rs` (`handle_input`)

**4. Route lane。** `ConversationIngress` 以完整 `RouteKey` 建立容量 16 的有界 FIFO lane。同 route 严格串行，不同 route 独立运行，不再有 shard hash 碰撞造成的 head-of-line blocking。`Stop` 会提升 stop generation、取消当前 turn 并丢弃此前排队的 prompt；daemon shutdown 先关闭 ingress 并等待 lane drain。
→ `src/core/src/channels/prompt/ingress.rs`

**5. Command parse。** 文本按 slash-command grammar 检查（`/new`、`/close`、`/switch`、`/pickup`、`/status`、resource commands、`/va` prefix forms）。命令在 workspace-thread layer 上执行，并以 system text 回复。Dormant 群聊路径保留 mention 防御检查；当前支持面是 DM/Web。
→ `src/core/src/channels/prompt/handler.rs` (`parse_thread_command`, `handle_command`)

**6. Route → thread runtime。** `resolve_route_runtime` 查 route 的 attachment：已附着的 open thread → 对应 runtime；没有 attachment → 创建 default workspace、持久化一个新 thread event、把 route 附着上去。升级旧插件后的第一条 extended-route 消息会在 migration lock 下接管并分离 legacy `(kind, kind, chat)` attachment。不同 instance、actor 与 topic 可映射到不同 thread；host runtime registry 与 SDK renderer 已按扩展 route/target 隔离，但 settings/UI 仍只暴露每种 channel kind 一个配置实例。
→ `src/core/src/workspace/manager.rs` (`resolve_route_runtime`)

**7. Ensure agent + session。** `ThreadRuntime` 将持久 session identity 与 live `AcpSessionRunner` 分开；runner 拥有一代 Agent 与 handler。死掉的一代会整体替换。Agent spawn 注册到 supervisor（restart policy `Never`）；在 ACP initialize 成功前由 cancellation-safe pending owner 持有 registration，Stop/abort 会自动 unregister 并 reap child；之后再创建或 resume 已记录的 CLI session。IM attachment 可 rehydrate，但不会重放旧 output。
→ `src/core/src/workspace/threads/runtime.rs` (`ensure_agent`, `ensure_session`)

**8. Prompt。** 文本 + attachment resource links 转成 ACP content blocks，发送 `session/prompt`。完整 route lane 与 thread prompt lock 同时生效。只有拿到 prompt lock 后，`ThreadRuntime` 才安装本 turn 的临时 `ChannelTarget`：持久 route 加入站平台 message id 对应的 `replyTo`；generation guard 会在正常完成、取消、报错或 task drop 时清除它。
→ `runtime.rs` (`prompt`), `src/core/src/channels/prompt/mod.rs` (content blocks)

**9. Notifications → outputs。** Host agent 的每个 ACP `session_notification` 都包成 thread reply，再实时发送给 thread 附着的 routes；即使 turn 中途 attachment 改变，当前 origin 也会保留。只有 origin 携带临时 `replyTo`，其它附着界面只收到 live thread event。SDK 以完整 `(instance, actor, chat, topic, replyTo)` 隔离 renderer 与 delivery state。
→ `src/core/src/channels/bridge_handler.rs` (`session_notification`)

**10. Output → chat。** `PluginHost` 按 `channel_instance_id` 把 output 路由到当前 live plugin runtime。有界内存缓冲负责背压，但 IM 输出不落盘、不在重启后 replay。Runtime 不存在或已断开时，当前 output 被丢弃并记录；无法投递的 permission 会被取消，避免 Agent 永久等待。
→ `src/core/src/channels/plugin_host.rs` (`send_output`), `plugin_runner.rs`

**尾声。** Turn 结束后：`PromptDone`（typing indicator 关闭），错误以 `❌` system text 发送（auth errors 会自动关闭 thread），并为 host agent 安排 10 分钟 idle shutdown。Thread 和 session id 持久保留；下一条消息会透明 respawn。
→ `src/core/src/channels/prompt/mod.rs` (`handle_prompt_input`), `manager.rs` (idle shutdown)

## 路径上的失败行为

| 失败 | 结果 |
|---|---|
| Plugin 在第 2 步前崩溃 | 平台可能重投；supervisor respawn plugin |
| Daemon 在第 4 到 8 步之间重启 | 正在执行的 turn 丢失；thread + session 持久保留并可 resume |
| 第 7 步 agent spawn 失败 | `❌` system text；需要 auth 的错误会自动关闭 thread |
| Agent 在第 8 步 turn 中崩溃 | Turn 报错；下一次 prompt 会 fresh spawn 并 resume session |
| Plugin 启动后冻结 | heartbeat watchdog 会重启完整 generation；忽略 cancel 的 bridge 会在 respawn 前被 tree-kill 与有界 abort |
| 第 10 步 plugin 已死 | 当前 output 丢弃；permission waiter 取消；重启后不 replay |

## 已验证 smoke 路径

2026-07-11 分支已经通过隔离 daemon、真实 WS 与已登录 Slack 客户端验证：

- WS 多轮会话可以记住只在第一轮出现的 token，`/status` 正常；
- 两条 route 的长回复真实重叠且输出互不串线；
- Slack 的 help/status/switch、真实多轮、插件 `SIGKILL` 后自动拉起与 session 续接均通过；
- Discord 本地协议与 build 测试通过；因为当前 bot 未挂到目标 server，真实平台验证按用户决定延期。

---

*Source anchors: `src/core/src/channels/` (types, plugin_runner, transport_stdio, plugin_host, bridge_handler, prompt/), `src/server/src/lib.rs` (input dispatcher/shutdown), `src/core/src/workspace/manager.rs` + `threads/runtime.rs`。*
*Last verified: `codex/im-acp-route-refactor` at `4a27a1c0`; Channel SDK `ae322ed`; WeCom `b495459`; Weixin `78bb3b8`（2026-07-12）。*

<sub>[◀ Internals](../README.md) · [文档索引](../../README.md) · [Flow: Web Chat ▶](web-chat.md)</sub>
