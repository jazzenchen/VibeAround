# 连接钉钉

钉钉使用 Stream Mode（WebSocket）—— 不需要公网回调 URL。

## 平台侧准备

1. 在[钉钉开放平台](https://open.dingtalk.com/)创建应用。
2. 在应用设置里启用 **Stream Mode**。
3. 复制 Client ID 和 Client Secret。
4. 在应用权限列表里授予消息事件相关权限。

## 配置

必填字段：`client_id`、`client_secret`。

```json
{
  "channels": {
    "dingtalk": {
      "client_id": "your-client-id",
      "client_secret": "your-client-secret",
      "verbose": { "show_thinking": true, "show_tool_use": true }
    }
  }
}
```

> 旧版指南把这两个字段叫 `app_key` / `app_secret` —— 插件要求的是 `client_id` / `client_secret`。

## 行为

- 什么都收不到？通常是 Stream Mode 没启用。

---

*Source anchors: [va-plugin-channel-dingtalk](https://github.com/jazzenchen/va-plugin-channel-dingtalk) `src/main.ts` (requiredConfig: client_id, client_secret).*
*Last verified: v0.7.11*

<sub>[◀ 连接渠道](../connect-channels.md) · [文档索引](../../README.md)</sub>
