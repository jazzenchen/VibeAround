# 连接 Slack

Slack 走 Socket Mode —— 不需要公网回调 URL。整个应用定义一次 manifest 粘贴就够。

## 平台侧准备

1. 在 [api.slack.com/apps](https://api.slack.com/apps) 通过 **Create New App → From an app manifest** 创建应用。
2. 粘贴这份 manifest：

```json
{
  "display_information": { "name": "VibeAround" },
  "features": {
    "bot_user": { "display_name": "VibeAround", "always_online": true },
    "slash_commands": [
      {
        "command": "/va",
        "description": "VibeAround command (e.g. /va help, /va switch claude)",
        "should_escape": false
      },
      {
        "command": "/vibearound",
        "description": "VibeAround command",
        "should_escape": false
      }
    ]
  },
  "oauth_config": {
    "scopes": {
      "bot": [
        "files:read", "app_mentions:read", "chat:write", "commands",
        "im:history", "im:read", "im:write"
      ]
    },
    "pkce_enabled": false
  },
  "settings": {
    "event_subscriptions": { "bot_events": ["app_mention", "message.im"] },
    "interactivity": { "is_enabled": true },
    "org_deploy_enabled": false,
    "socket_mode_enabled": true,
    "token_rotation_enabled": false,
    "is_mcp_enabled": false
  }
}
```

3. 为 Socket Mode 生成一个带 `connections:write` scope 的 **App-Level Token**（`xapp-…`）。
4. 把应用安装到工作区，复制 **Bot User OAuth Token**（`xoxb-…`）。
5. 频道里：邀请应用并 @提及它。私聊：直接发消息。

## 配置

必填字段：`bot_token`（xoxb）和 `app_token`（xapp）。

```json
{
  "channels": {
    "slack": {
      "bot_token": "xoxb-...",
      "app_token": "xapp-...",
      "verbose": { "show_thinking": true, "show_tool_use": true }
    }
  }
}
```

## 行为

- Slack 保留了裸的斜杠命令，所以 VibeAround 命令用 `/va` 前缀：`/va new`、`/va switch claude`、`/va status`（[命令参考](../im-usage.md#命令参考) —— 每条命令都能加前缀使用）。
- 连接一建立就断？确认 Socket Mode 已启用，且 `app_token` 是 `xapp-` 开头的 token，不是 bot token。

---

*Source anchors: [va-plugin-channel-slack](https://github.com/jazzenchen/va-plugin-channel-slack) `src/main.ts` (requiredConfig); manifest verified against the plugin's expected scopes/events.*
*Last verified: v0.7.11*

<sub>[◀ 连接渠道](../connect-channels.md) · [文档索引](../../README.md)</sub>
