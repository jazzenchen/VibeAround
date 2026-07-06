# 连接飞书 / Lark

飞书和 Lark 共用一套开放平台模型，但开发者后台和 API 域名必须与你的租户匹配：中国大陆的飞书用[飞书开放平台](https://open.feishu.cn/)（`open.feishu.cn`）；海外的 Lark 用 [Lark Developer](https://open.larksuite.com/)（`open.larksuite.com`）。应用、权限、事件/回调订阅、以及你粘贴进 VibeAround 的凭据必须**全部在同一个平台上** —— 飞书应用配 Lark 域名（或反过来）会连不上。

VibeAround 的事件和回调都走**长连接**（由你的机器发起的 WebSocket），所以不需要公网回调 URL 或隧道。

## 平台侧准备

1. 打开对应的开发者后台，创建一个**企业自建应用**。
2. 启用**机器人能力**，复制 App ID 和 App Secret。
3. 在 *开发配置 → 权限管理* 里，用下面的权限 JSON **批量导入**。
4. 在 *开发配置 → 事件与回调 → 事件配置* 里选择**长连接**，添加 `im.message.receive_v1`（把用户消息投递给 VibeAround）。
5. 在 *回调配置* 里选择**长连接**，添加 `card.action.trigger`（投递卡片按钮点按 —— 批准/拒绝、会话选择）。
6. 在 *版本管理与发布* 里**创建并发布一个版本** —— 权限、事件和回调通常发布后才生效，企业租户可能还需要管理员审批。
7. 先把机器人加进一个可控的私聊或小群；确认消息送达和卡片按钮都正常，再大范围推开。

权限批量导入 JSON：

```json
{
  "scopes": {
    "tenant": [
      "aily:file:read",
      "aily:file:write",
      "application:application.app_message_stats.overview:readonly",
      "application:application:self_manage",
      "application:bot.menu:write",
      "cardkit:card:write",
      "contact:user.employee_id:readonly",
      "corehr:file:download",
      "event:ip_list",
      "im:chat.access_event.bot_p2p_chat:read",
      "im:chat.members:bot_access",
      "im:message",
      "im:message.group_at_msg:readonly",
      "im:message.p2p_msg:readonly",
      "im:message:readonly",
      "im:message:send_as_bot",
      "im:resource"
    ],
    "user": [
      "aily:file:read",
      "aily:file:write",
      "im:chat.access_event.bot_p2p_chat:read"
    ]
  }
}
```

## 配置

必填字段：`app_id`、`app_secret`。

```json
{
  "channels": {
    "feishu": {
      "app_id": "cli_xxxxxxxxxxxx",
      "app_secret": "your-app-secret",
      "verbose": { "show_thinking": true, "show_tool_use": true }
    }
  }
}
```

## 行为

- 回复渲染为 **V2 交互卡片**，随 Agent 流式输出就地更新；权限请求以可点按的卡片按钮送达。
- 群聊和私聊是独立的对话 Route；一个群里的多个 bot 各自维护自己的 Thread。

## 故障排查

| 症状 | 检查 |
|---|---|
| 收不到消息 | 事件配置用的是长连接，且包含 `im.message.receive_v1` |
| 卡片按钮没反应 | 回调配置用的是长连接，且包含 `card.action.trigger` |
| 权限导入了还是报错 | 发布一个新的应用版本；确认管理员审批已通过 |
| 长连接校验失败 | 先启动 VibeAround（连接由插件发起）；确认机器能访问对应的开放平台域名 |
| 凭据连不上 | App ID/Secret、开发者后台、API 域名必须属于同一个平台（飞书 vs Lark） |

官方参考：[API 权限](https://open.feishu.cn/document/server-docs/application-scope/introduction) · [长连接事件订阅](https://open.feishu.cn/document/server-docs/event-subscription-guide/event-subscription-configure-/request-url-configuration-case) · [长连接回调](https://open.feishu.cn/document/event-subscription-guide/callback-subscription/step-1-choose-a-subscription-mode/configure-callback-request-address) · [接收消息事件](https://open.larksuite.com/document/uAjLw4CM/ukTMukTMukTM/reference/im-v1/message/events/receive) · [卡片交互](https://open.feishu.cn/document/feishu-cards/configuring-card-interactions)

---

*Source anchors: [va-plugin-channel-feishu](https://github.com/jazzenchen/va-plugin-channel-feishu) `src/main.ts` (requiredConfig: app_id, app_secret); permission JSON and subscription names from the verified website draft (2026-06); V2 card requirement per the Feishu platform.*
*Last verified: v0.7.11*

<sub>[◀ 连接渠道](../connect-channels.md) · [文档索引](../../README.md)</sub>
