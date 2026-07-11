# Flow: 权限请求

从“agent 想运行命令”到“你点了 Allow”之间发生了什么。这个流程是安全关键路径：它的不变量是权限请求一定会终止，即 approved、denied 或 cancelled，绝不会静默丢失。

## 逐跳

```text
agent ──ACP requestPermission──► bridge handler
                                     │ register oneshot (request_id)
                                     ▼
                              PermissionRequest output ──► plugin ──► card in chat
                                                                          │ tap
agent ◄──ACP response── bridge handler ◄──oneshot── forwarder ◄── callback with request_id
```

**1. Agent 请求。** Turn 中途，agent CLI 发送 ACP `session/request_permission`，携带选项（allow once、always、reject 等）。Agent 的 turn 此时阻塞在回复上。
→ `src/core/src/agent/runtime.rs` (client handler trait)

**2. Bridge handler 捕获 active origin 并注册 oneshot。** Thread 的 `ChannelBridgeHandler` 读取当前 turn 的 `ChannelTarget`；没有 active target 时立即向 ACP 返回 **Cancelled**。否则生成新的 `request_id`，把 origin channel instance 与 `oneshot::Sender` 存进 `PluginHost::pending_permissions`，只向这个 origin 发出一条 target-aware `ChannelOutput::PermissionRequest { request_id, payload }`。Live card 等人操作时**故意没有 timeout**。
→ `src/core/src/channels/bridge_handler.rs` (`request_permission`), `plugin_host.rs` (`pending_permissions`)

**3. Card 渲染。** Plugin 把 payload 转成平台原生互动卡片（飞书 V2 card、Slack block actions、Telegram inline keyboard）。IM 只做实时投递：目标 runtime 不存在或无法接收时立即移除该 surface；没有其它可响应 surface 时取消 waiter。Web 上，chat 渲染卡片组件并标记为 pending。
→ plugin repos; `src/core/src/channels/plugin_host.rs`; `src/server/src/web_server/ws_chat.rs` (web cards)

**4. 点按回流。** 两条返回路径进入同一张表：
- **Stdio plugins：** 点按作为 ACP response 通过 plugin bridge forwarder 回来，pop `pending_permissions[request_id]` 并触发 oneshot。
- **Web chat：** 浏览器通过 `/va/ws/chat` 发送 typed `PermissionResponse`；handler 调 `respond_permission(channel_instance_id, request_id, response)`，先验证该 request 属于该 surface，再触发。
→ `src/core/src/channels/transport_stdio/` (forwarder), `plugin_host.rs` (`respond_permission`)

**5. Agent 继续。** Bridge handler 的 `rx.await` 得到所选选项，并把它作为 ACP response 返回。Agent 随后继续或中止 tool call。

## 为什么不会永远卡住

无 timeout 的设计需要清理保证：

| 情况 | 保证 |
|---|---|
| 没有 live runtime 接收 card | 立即移除该 instance；没有其它 surface 时，`rx.await` 报错并以 **Cancelled** 回复 agent |
| Card pending 时 plugin 进程死亡或 bridge task 被强制 abort | Generation-scoped Drop guard 仅移除本代 runtime，并调用 `cancel_channel_permissions(instance_id)`，pending sender 随即 drop |
| Card pending 时用户发送 `/stop` | 被取消的 prompt drop 其 RAII registration，core entry 随即删除；SDK 同时用最安全 reject option 完成旧 permission 并移除两个 callback index，下一条文本不会被旧卡片吞掉 |
| Daemon shutdown | `PluginHost::shutdown_all` 先清空整张表，走同一个 cancellation path |
| 对已经 resolved 的 request 再次点按 | `respond_permission` 找不到 entry，返回“不再 pending”，第二次点按是 no-op，不会 double-approve |
| 来自错误 instance 的点按 | Instance membership check 保留 entry 并拒绝 response |

Host turn 的最终不变量是：**每个注册的 oneshot 都会刚好被消费或 drop 一次**，来源是点按、prompt cancel/drop、channel death 或 shutdown。Agent turn 可以无限等一个 live human surface，但不能等已死进程，同 thread 的其它 route 也不能代为批准。

> 剩余范围：subagent permission handling 仍走独立 fan-out 路径，尚未由 host turn 的 `ActiveTurnTarget` 约束，其 core pending registration 也没有同等的 cancellation-safe RAII guard。应在 subagent session ownership 重构时一并解决 origin 与 cleanup。

---

*Source anchors: `src/core/src/channels/bridge_handler.rs` (request_permission), `src/core/src/channels/plugin_host.rs` (pending_permissions, respond_permission, cancel_channel_permissions, shutdown_all), `src/core/src/channels/transport_stdio/` (forwarder), `src/server/src/web_server/ws_chat.rs` (web response path).*
*Last verified: `codex/im-acp-route-refactor` at `ed12aa02`（2026-07-11）。*

<sub>[◀ Flow: Web Chat](web-chat.md) · [文档索引](../../README.md) · [Flow: Bridge 请求 ▶](bridge-request.md)</sub>
