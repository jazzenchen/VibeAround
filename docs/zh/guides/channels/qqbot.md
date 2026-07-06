# 连接 QQ 机器人

## 平台侧准备

1. 在 [QQ 开放平台](https://q.qq.com/)创建机器人。
2. 复制 App ID 和 secret。
3. 把机器人加进目标频道服务器和子频道。

## 配置

必填字段：`app_id`、`secret`。

```json
{
  "channels": {
    "qqbot": {
      "app_id": "your-app-id",
      "secret": "your-secret",
      "verbose": { "show_thinking": false, "show_tool_use": false }
    }
  }
}
```

> 旧版指南把第二个字段叫 `app_token` —— 插件要求的是 `secret`。

## 行为

- 在 QQ 频道内回复是只发送模式（没有就地流式编辑）。

---

*Source anchors: [va-plugin-channel-qqbot](https://github.com/jazzenchen/va-plugin-channel-qqbot) `src/main.ts` (requiredConfig: app_id, secret).*
*Last verified: v0.7.11*

<sub>[◀ 连接渠道](../connect-channels.md) · [文档索引](../../README.md)</sub>
