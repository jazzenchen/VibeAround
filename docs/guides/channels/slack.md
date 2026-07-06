# Connect Slack

Slack runs over Socket Mode — no public callback URL needed. The whole app definition fits in one manifest paste.

## Platform setup

1. Create an app at [api.slack.com/apps](https://api.slack.com/apps) via **Create New App → From an app manifest**.
2. Paste this manifest:

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

3. Generate an **App-Level Token** (`xapp-…`) with the `connections:write` scope for Socket Mode.
4. Install the app to your workspace and copy the **Bot User OAuth Token** (`xoxb-…`).
5. In a channel: invite the app and @mention it. In DMs: just message it.

## Configuration

Required fields: `bot_token` (xoxb) and `app_token` (xapp).

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

## Behavior

- Slack reserves bare slash commands, so VibeAround commands use the `/va` prefix: `/va new`, `/va switch claude`, `/va status` ([command reference](../im-usage.md#command-reference) — every command works behind the prefix).
- Connection dies immediately? Check that Socket Mode is enabled and `app_token` is the `xapp-` token, not the bot token.

---

*Source anchors: [va-plugin-channel-slack](https://github.com/jazzenchen/va-plugin-channel-slack) `src/main.ts` (requiredConfig); manifest verified against the plugin's expected scopes/events.*
*Last verified: v0.7.11*

<sub>[◀ Connect channels](../connect-channels.md) · [Documentation index](../../README.md)</sub>
