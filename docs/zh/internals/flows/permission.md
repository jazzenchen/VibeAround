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

**2. Bridge handler 注册 oneshot。** Thread 的 `ChannelBridgeHandler` 生成新的 `request_id`，把 `(channel_kind, oneshot::Sender)` 存进 `PluginHost::pending_permissions`，并向该 thread 附着的 routes 发出 `ChannelOutput::PermissionRequest { request_id, payload }`。这里**故意没有 timeout**，UX contract 是“用户可以慢慢决定”。
→ `src/core/src/channels/bridge_handler.rs` (`request_permission`), `plugin_host.rs` (`pending_permissions`)

**3. Card 渲染。** Plugin 把 payload 转成平台原生互动卡片（飞书 V2 card、Slack block actions、Telegram inline keyboard）。权限请求是 outbox-durable：如果 plugin 已死，respawn 后再投递卡片。Web 上，chat 渲染卡片组件并标记为 pending。
→ plugin repos; `src/core/src/channels/outbox.rs`; `src/server/src/web_server/ws_chat.rs` (web cards)

**4. 点按回流。** 两条返回路径进入同一张表：
- **Stdio plugins：** 点按作为 ACP response 通过 plugin bridge forwarder 回来，pop `pending_permissions[request_id]` 并触发 oneshot。
- **Web chat：** 浏览器通过 `/va/ws/chat` 发送 typed `PermissionResponse`；handler 调 `respond_permission(channel_kind, request_id, response)`，先验证该 request 属于该 channel，再触发。
→ `src/core/src/channels/transport_stdio/` (forwarder), `plugin_host.rs` (`respond_permission`)

**5. Agent 继续。** Bridge handler 的 `rx.await` 得到所选选项，并把它作为 ACP response 返回。Agent 随后继续或中止 tool call。

## 为什么不会永远卡住

无 timeout 的设计需要清理保证：

| 情况 | 保证 |
|---|---|
| Card pending 时 plugin 进程死亡 | 死亡的 bridge 会刚好调用一次 `cancel_channel_permissions(kind)`，pending senders 被 drop，`rx.await` 报错，handler 以 **Cancelled** 回复 agent |
| Daemon shutdown | `PluginHost::shutdown_all` 先清空整张表，走同一个 cancellation path |
| 对已经 resolved 的 request 再次点按 | `respond_permission` 找不到 entry，返回“不再 pending”，第二次点按是 no-op，不会 double-approve |
| 来自错误 channel 的点按 | Channel-kind check 会把 entry 插回去并拒绝 response |

最终不变量：**每个注册的 oneshot 都刚好消费一次**，消费方要么是点按，要么是 channel cancellation，要么是 shutdown。Agent turn 可以无限等人，但不能等一个已死进程。

> 已知缺口：pending entry 只标了**第一个**附着 route 的 channel kind，而 card 会 fan out 到所有附着 route。多界面 thread（handover）里，其它界面的点按会被 channel check 拒绝，而且 wrong-channel re-insert 可能和 plugin-death drain 竞态。remediation plan 中以 H13 跟踪。

---

*Source anchors: `src/core/src/channels/bridge_handler.rs` (request_permission), `src/core/src/channels/plugin_host.rs` (pending_permissions, respond_permission, cancel_channel_permissions, shutdown_all), `src/core/src/channels/transport_stdio/` (forwarder), `src/core/src/channels/outbox.rs` (durability), `src/server/src/web_server/ws_chat.rs` (web response path).*
*Last verified: v0.7.11*

<sub>[◀ Flow: Web Chat](web-chat.md) · [文档索引](../../README.md) · [Flow: Bridge 请求 ▶](bridge-request.md)</sub>
