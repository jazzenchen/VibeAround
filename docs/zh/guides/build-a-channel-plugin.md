# 开发渠道插件

渠道插件把一个 IM 平台接入 VibeAround：把平台事件变成渠道信封（envelope），把渠道输出变成平台消息。本页带你从零跑起一个插件；它接入的架构见[渠道插件系统](../architecture/channel-plugin-system.md)。

## 你要做的东西

一个 Node.js 进程，三项职责：

1. **监听**你的平台（webhook、长轮询或 WebSocket —— 随你选，守护进程不关心）。
2. **入站翻译**：平台消息 → 渠道信封（route key、消息 id、发送者、文本、附件）→ 发给守护进程。
3. **出站翻译**：守护进程的输出（Agent 文本、系统消息、回合状态、权限请求）→ 平台消息/卡片；按钮点按 → 带 request id 的回调。

其余一切由守护进程负责：路由、Thread、Agent、顺序、持久化，以及你的进程的生命周期（拉起、崩溃重启、心跳看门狗）。

## 从 SDK 开始

```bash
npm i @vibearound/plugin-channel-sdk
```

SDK 封装了 stdio ACP 传输：连接握手、信封/输出类型、心跳发送，以及权限卡片往返的辅助函数。参考一个现有插件（[va-plugin-channel-telegram](https://github.com/jazzenchen/va-plugin-channel-telegram) 是最小的现实参考；全部十个都链接在[支持矩阵](../product/supported-matrix.md#im-渠道)里）和 [SDK 仓库](https://github.com/jazzenchen/va-plugin-channel-sdk)（发布为 `@vibearound/plugin-channel-sdk`）看完整的消息形状。

### 你实际要处理的线上格式

两个方向都是带 `kind` 标签的 JSON，字段用 camelCase，`route`/`attachments` 内部用 snake_case。入站用户消息（插件 → 守护进程）：

```json
{
  "kind": "message",
  "route": { "channel_kind": "telegram", "bot_id": "my_bot", "chat_id": "chat_42" },
  "messageId": "msg_1001",
  "text": "fix the failing test",
  "senderId": "user_7",
  "attachments": [
    { "message_id": "msg_1001", "file_key": "upload_ab12", "file_name": "log.txt", "resource_type": "text/plain", "size": 5120 }
  ]
}
```

按钮点按以回调形式返回（`text` 为空，选择放在 `actionValue`）：

```json
{ "kind": "callback", "route": { … }, "messageId": "msg_1002", "text": "", "actionValue": "allow_once" }
```

你必须渲染成卡片的权限请求（守护进程 → 插件）—— 通过 SDK 的 `client.requestPermission` 处理器，用同一个 `requestId` 回答它：

```json
{
  "kind": "permissionRequest",
  "route": { … },
  "requestId": "perm_9f3e",
  "payload": { "…": "JSON 序列化的 ACP RequestPermissionRequest：工具调用信息 + 选项" }
}
```

其余可渲染或忽略的输出：`threadReply`（流式 Agent 输出）、`systemText`、`turnStatus`（输入中指示的开/关）、`promptDone`、`sessionInfo` / `sessionMode` / `commandMenu`（更丰富的 UI）、`multiAgentTurn` / `subagentStatus`（多 Agent 进度）。可发送的控制输入：`stop`、`close`、`log`。

> 版本规则：依赖已发布的 SDK 版本（`^x.y.z`）。永远不要把插件固定在本地 `file:` 路径上发布。

## 清单与目录布局

插件是 `~/.vibearound/plugins/<kind>/` 下的一个目录，清单里声明：

- `kind: "channel"` 和渠道 id（你的 route key 里的 `channel_kind`），
- 守护进程拉起的 Node 入口，
- 配置 schema —— 用户需要在 `settings.json` 的 `channels.<your-kind>` 下填什么。

守护进程把用户的 `channels.<kind>` 对象原样传给你的进程；启动时校验它，不可用就带清晰错误退出（监督器会把崩溃循环呈现给用户）。

## 插件必须遵守的契约

- **心跳。** 按 SDK 的节奏发 `_va/heartbeat`（每 15 秒）。90 秒没心跳，守护进程会认定你冻结并重启你 —— 所以不要让平台调用阻塞事件循环。
- **Route key 就是身份。** `(channel_kind, bot_id, chat_id)` 对每个对话必须稳定：同一聊天 → 同一 key，不同聊天 → 不同 key。知道真实 bot id 后要上报；一个群里有多个 bot 时全靠它。
- **权限请求必须有结果。** 渲染成交互卡片/按钮；把用户的选择带着同一个 `request_id` 发回来。平台渲染不了按钮就退化为文本提示并解析回复 —— 绝不能悄悄丢掉请求。
- **回调携带 action value。** 按钮点按变成带该 action 值的回调；守护进程把它变成用户的回答（信封没有文本时则作为提示）。
- **附件用安全引用。** 把平台文件下载到共享缓存并传 file key；带路径分隔符或 `..` 的 key 会被守护进程拒绝。
- **致命错误退出，其余永远运行。** 不可恢复的配置错误：记日志并退出（用户看到干净的失败）。平台的暂时性错误：内部重试；你的进程应该是长命的。

## 对着运行中的守护进程开发

1. 运行守护进程（桌面应用或 `va serve`）。
2. 把开发中的插件（带清单）放进用户插件目录（或开发期间的项目插件目录）。
3. 在 `channels.<your-kind>` 下加最小配置，然后 `va channel sync`。
4. 迭代：改完 `va channel restart <your-kind>`；`va channels` 看状态；守护进程日志会显示你的 stderr，带渠道 kind 标签。
5. 按[连接渠道](connect-channels.md#验证新渠道)的清单验证：消息 → 回复、`/status`、权限卡片往返、双向附件。

## 发布

插件以独立仓库/npm 包的形式发行，由桌面插件管理器装进插件目录。你的 README 必须覆盖用户需要的平台侧准备：bot 创建、token/权限、webhook 配置，以及平台的已知限制（卡片 schema 版本、消息大小、频率限制 —— 例如飞书卡片必须使用 V2 卡片 schema）。

---

*Source anchors: `src/core/src/plugins/` (manifest, discovery dirs), `src/core/src/channels/transport_stdio/` (wire protocol), `src/core/src/channels/types.rs` (envelope/output shapes), `src/core/src/channels/monitor.rs` + `process/supervisor.rs` (heartbeat, respawn), `src/core/src/routing.rs` (route keys, attachment key rules); SDK: `@vibearound/plugin-channel-sdk`.*
*Last verified: v0.7.11*

<sub>[◀ 隧道与远程访问](tunnels-and-remote-access.md) · [文档索引](../README.md) · [源码构建 ▶](build-from-source.md)</sub>
