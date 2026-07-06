# 连接 Discord

一个 bot token 加两处开发者门户的开关；bot 在被 @ 的地方应答。

## 平台侧准备

1. 在 [Discord Developer Portal](https://discord.com/developers/applications) 创建应用，在 **Bot** 下生成 token。
2. 启用 Privileged Gateway Intents：**Message Content Intent** 必开；Server Members Intent 可选。
3. 在 **OAuth2 → URL Generator** 选择 scope `bot`，权限勾选 `Send Messages`、`Read Message History`、`Embed Links`。
4. 打开生成的邀请 URL，把 bot 加进你的服务器。

## 配置

必填字段：`bot_token`。

```json
{
  "channels": {
    "discord": {
      "bot_token": "your-discord-bot-token",
      "verbose": { "show_thinking": true, "show_tool_use": true }
    }
  }
}
```

## 行为

- 在服务器频道里，bot 只在被 **@提及** 时响应；每个提及它的频道成为独立的对话 Route。
- 完全收不到消息？通常是 Message Content Intent 没开。

---

*Source anchors: [va-plugin-channel-discord](https://github.com/jazzenchen/va-plugin-channel-discord) `src/main.ts` (requiredConfig).*
*Last verified: v0.7.11*

<sub>[◀ 连接渠道](../connect-channels.md) · [文档索引](../../README.md)</sub>
