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

**2. Bridge handler 注册 oneshot。** Thread 的 `ChannelBridgeHandler` 生成新的 `request_id`，把可响应的 channel instance id 集合和 `oneshot::Sender` 存进 `PluginHost::pending_permissions`，并向该 thread 附着的 routes 发出 `ChannelOutput::PermissionRequest { request_id, payload }`。Card 仍在线时，用户决定过程**故意没有 timeout**。
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
| Card pending 时 plugin 进程死亡 | 死亡的 bridge 会刚好调用一次 `cancel_channel_permissions(instance_id)`；没有其它 surface 时 drop sender |
| Daemon shutdown | `PluginHost::shutdown_all` 先清空整张表，走同一个 cancellation path |
| 对已经 resolved 的 request 再次点按 | `respond_permission` 找不到 entry，返回“不再 pending”，第二次点按是 no-op，不会 double-approve |
| 来自错误 instance 的点按 | Instance membership check 保留 entry 并拒绝 response |

最终不变量：**每个注册的 oneshot 都刚好消费一次**，消费方要么是点按，要么是 channel cancellation，要么是 shutdown。Agent turn 可以无限等人，但不能等一个已死进程。

> 剩余安全缺口：permission 仍会 fan out 到 workspace thread 附着的所有 route，第一个合法响应获胜。下一步 target-aware turn 改造会把 permission 限制到当前 turn 的 origin target。

---

*Source anchors: `src/core/src/channels/bridge_handler.rs` (request_permission), `src/core/src/channels/plugin_host.rs` (pending_permissions, respond_permission, cancel_channel_permissions, shutdown_all), `src/core/src/channels/transport_stdio/` (forwarder), `src/server/src/web_server/ws_chat.rs` (web response path).*
*Last verified: v0.7.11*

<sub>[◀ Flow: Web Chat](web-chat.md) · [文档索引](../../README.md) · [Flow: Bridge 请求 ▶](bridge-request.md)</sub>
