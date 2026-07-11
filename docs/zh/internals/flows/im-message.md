# Flow: IM 消息

跟踪 Telegram/飞书/Slack 聊天里的一条消息，从平台事件一直到流式回复。这是主干流程，[Web Chat](web-chat.md) 和 [权限请求](permission.md) 都从它分叉。文件引用均为仓库相对路径；行级细节在锚定文件的 module docs 里。

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

**1. Platform → plugin。** Channel plugin（独立 Node.js 进程）收到 webhook/long-poll event，把附件下载到 `~/.vibearound/.cache/`，构建 `ChannelEnvelope`：route key `(channel_kind, bot_id, chat_id)`、message id、sender、text、attachment refs。
→ plugin repo；envelope type 在 `src/core/src/channels/types.rs`

**2. Plugin → daemon。** Envelope 通过 stdio 作为 JSON-RPC notification（ACP framing）传入，由 plugin bridge 解码成 `ChannelInput::Message`。
→ `src/core/src/channels/transport_stdio/`, `plugin_bridge.rs`

**3. Enqueue。** `ChannelManager::handle_input` 是 fire-and-forget：input 进入 unbounded mpsc queue。任何面向平台的代码都不会等待 agent 工作。
→ `src/core/src/channels/mod.rs` (`handle_input`)

**4. Route lane。** `ConversationIngress` 以完整 `RouteKey` 建立容量 16 的有界 FIFO lane。同 route 严格串行，不同 route 独立运行，不再有 shard hash 碰撞造成的 head-of-line blocking。`Stop` 会提升 stop generation、取消当前 turn 并丢弃此前排队的 prompt；daemon shutdown 先关闭 ingress 并等待 lane drain。
→ `src/core/src/channels/prompt/ingress.rs`

群聊地址语义由插件先提取、core 再防御性校验：DM 不要求 @；group text 必须 @ 当前 bot；callback 属于显式交互。Route contract 已包含 `bot_id/actor_id/topic_id`，但官方插件尚未端到端发送扩展 metadata，因此 multi-bot/multi-actor 仍是部分落地。

**5. Command parse。** 文本按 slash-command grammar 检查（`/new`、`/close`、`/switch`、`/pickup`、`/status`、resource commands、`/va` prefix forms）。命令在 workspace-thread layer 上执行，并以 system text 回复；对命令来说流程到这里结束。
→ `src/core/src/channels/prompt/handler.rs` (`parse_thread_command`, `handle_command`)

**6. Route → thread runtime。** `resolve_route_runtime` 查 route 的 attachment：已附着的 open thread → 对应 runtime；没有 attachment → 创建 default workspace、持久化一个新 thread event、把 route 附着上去。IM route 的 default workspace 是 `<default_workspace>/im/<channel_kind>`（kind 里的路径分隔符会清理成 `_`）；host agent/profile 来自 `remote.channels.<kind>` defaults，fallback 到 global default。per-route lock 让并发下的这一步幂等。
→ `src/core/src/workspace/manager.rs` (`resolve_route_runtime`)

**7. Ensure agent + session。** Thread runtime 在没有 host agent 时启动它：物化 profile env，注入 `VIBEAROUND_*` context env，把进程注册到 supervisor（restart policy `Never`），交换 ACP `initialize`。然后确保 CLI session 存在：如果 thread 记录了 session id，就 resume 那个 session。
→ `src/core/src/workspace/threads/runtime.rs` (`ensure_agent`, `ensure_session`)

**8. Prompt。** 文本 + attachment resource links 转成 ACP content blocks，发送 `session/prompt`。同一 thread 上的 turn 会串行化（同一 thread 的第二条消息要等当前 turn 完成）。
→ `runtime.rs` (`prompt`), `src/core/src/channels/prompt/mod.rs` (content blocks)

**9. Notifications → outputs。** Agent 发出的每个 ACP `session_notification` 都包成 thread reply，再作为 `ChannelOutput` fan out 到**所有附着在该 thread 的 route**。这就是为什么交接后的另一个界面能实时看到同一个 turn。
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
| 第 10 步 plugin 已死 | 当前 output 丢弃；permission waiter 取消；重启后不 replay |

---

*Source anchors: `src/core/src/channels/` (types, plugin_runner, transport_stdio, plugin_host, bridge_handler, prompt/), `src/server/src/lib.rs` (input dispatcher/shutdown), `src/core/src/workspace/manager.rs` + `threads/runtime.rs`。*
*Last verified: `codex/im-acp-route-refactor` at `0ba7fa2e`（2026-07-11）。*

<sub>[◀ Internals](../README.md) · [文档索引](../../README.md) · [Flow: Web Chat ▶](web-chat.md)</sub>
