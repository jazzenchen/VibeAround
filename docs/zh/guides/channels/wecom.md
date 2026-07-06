# 连接企业微信

## 平台侧准备

1. 在企业微信管理后台创建机器人。
2. 复制 bot id 和 secret。

## 配置

必填字段：`bot_id`、`secret`。

```json
{
  "channels": {
    "wecom": {
      "bot_id": "your-bot-id",
      "secret": "your-bot-secret",
      "verbose": { "show_thinking": true, "show_tool_use": true }
    }
  }
}
```

> 旧版指南把第二个字段叫 `bot_secret` —— 插件要求的是 `secret`。

---

*Source anchors: [va-plugin-channel-wecom](https://github.com/jazzenchen/va-plugin-channel-wecom) `src/main.ts` (requiredConfig: bot_id, secret).*
*Last verified: v0.7.11*

<sub>[◀ 连接渠道](../connect-channels.md) · [文档索引](../../README.md)</sub>
