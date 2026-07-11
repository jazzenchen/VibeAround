# Module: channels

`src/core/src/channels/`：从“某个界面收到消息”到“thread runtime 收到 prompt”之间的一切，以及反向路径。经过它的流程：[IM 消息](../flows/im-message.md)、[Web Chat](../flows/web-chat.md)、[权限请求](../flows/permission.md)。

## 职责

托管 channel plugins（进程外 stdio 和进程内 websocket），把所有入站流量归一化成 `ChannelInput`，派发到 workspace-thread layer，并把每个 `ChannelOutput` 路由回应该渲染它的界面。它拥有消息的*传输和路由*，但从不拥有对话状态（那是 `workspace`），也不负责进程启动（那是 `process`）。

## 关键类型

| Type | File | Role |
|---|---|---|
| `ChannelManager` | `mod.rs` | Daemon 生命周期 facade：input queue、plugin registration、sync、shutdown |
| `ConversationIngress` | `prompt/ingress.rs` | stdio/Web/TUI 共用业务入口；完整 route 的有界 FIFO lane、Stop generation 与 shutdown barrier |
| `ChannelInput` / `ChannelOutput` / `ChannelEnvelope` | `types.rs` | 每个界面都使用的 wire vocabulary |
| `RouteKey` / `ChannelTarget` / `ActiveTurnTarget` | `routing.rs` | 持久对话 identity、单条消息的临时 delivery identity、可取消的当前 turn origin |
| `PluginHost` | `plugin_host.rs` | 路由表：channel instance → live runtime；pending-permissions table |
| `PluginRuntime` | `plugin_runtime.rs` | stdio / websocket runtime 的 enum |
| `ChannelPluginRunner` / factory | `plugin_runner.rs` | 一个受监管 stdio plugin generation 的协议 owner；每次 respawn 重建 |
| `ChannelMonitor` | `monitor.rs` | Dashboard 通过 supervisor 查看 plugin lifecycle 的 facade |
| `ChannelBridgeHandler` | `bridge_handler.rs` | 每个 thread 的 ACP client handler：notification fan-out + permission round-trip |
| Prompt handler | `prompt/handler.rs` | Lane 后的业务 dispatch：command parse → thread ops → prompt |

## 交互

- **← plugins/surfaces：** stdio plugins 通过 bridge；web/TUI 通过 `WebChannelManager` 注册的 senders。
- **→ workspace：** `prompt/handler.rs` 调 `WorkspaceThreadManager` 做 route resolution、commands、prompts。
- **→ process：** monitor 把 plugin manifests 注册给 `Supervisor`；bridge factory 在每次 respawn 后把 live runtime 重新注册进 `PluginHost`。
- **← agent：** `ChannelBridgeHandler` 从 hosted agents 接收 ACP notifications/permission requests，并转成 outputs。

## 不变量：不要破坏

1. **Per-route ordering** 属于 `ConversationIngress`；Web/TUI/stdio 的业务与控制路径都不能绕过它。
2. **`handle_input` 绝不阻塞**，它只是 queue send；面向平台的代码绝不能等待 agent 工作。
3. **每个 host-turn permission 都会终止**：请求只发送给 active origin；RAII registration 会被点按、prompt cancel/drop、bridge death 的 `cancel_channel_permissions` 或 `shutdown_all` 移除。
4. **IM output 只做实时投递**：stdio transport 有有界内存缓冲，但没有 durable queue；连接断开后的 output 不会在重启后 replay。
5. **Runtime ownership 按 instance 隔离**：heartbeat、output、permission cleanup、stop、restart 使用 `channel_instance_id`；discovery 和 platform traits 继续使用 `channel_kind`。
6. **群聊地址必须明确**：DM 不要求 @；group text 必须 @ 当前 bot；callback 属于显式交互。
7. `ChannelManager::shutdown_all` 只能停止 channel 自己持有的 supervised IDs，不能 drain 全局 supervisor。
8. **`replyTo` 只属于临时投递**：它可以选择平台回复目标和 SDK renderer lane，但不能进入 `RouteKey`、持久 attachment 或 workspace-thread 选择。

## 已知技术债

- 上游 `ChannelManager` input queue 仍是 unbounded；route lane 与 stdio plugin output 已有界。
- Web-chat session-intent side effects 仍早于 route lane serialization。
- Route/target contract 与 SDK renderer 已携带 instance/actor/topic 和单消息 `replyTo`，但 settings/UI 仍只暴露每种 channel kind 一个配置实例。
- `RouteKey::as_key()` 仍是有意保持兼容的有损 display/API key，不能作为 extended route identity。
- Runtime control 以 workspace thread id 列出和停止 host；legacy `kind:chat` 只有在唯一命中一个 live extended route 时才兼容接受。
- Host-turn permission 已限制到 origin 且 cancellation-safe；subagent permission 仍 fan-out，且缺少同等 RAII pending-registration cleanup。

---

*Source anchors: `src/core/src/channels/`，`src/server/src/lib.rs`（input dispatcher 与 ingress-first shutdown）。*
*Last verified: `codex/im-acp-route-refactor` at `ed12aa02`（2026-07-11）。*

<sub>[◀ Flow: PTY 终端](../flows/web-terminal.md) · [文档索引](../../README.md) · [Module: workspace ▶](workspace.md)</sub>
