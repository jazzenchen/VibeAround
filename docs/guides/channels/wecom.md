# Connect WeCom (企业微信)

## Platform setup

1. Create a bot in the WeCom admin console.
2. Copy the bot id and secret.

## Configuration

Required fields: `bot_id`, `secret`.

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

> Older guides named the second field `bot_secret` — the plugin requires `secret`.

---

*Source anchors: [va-plugin-channel-wecom](https://github.com/jazzenchen/va-plugin-channel-wecom) `src/main.ts` (requiredConfig: bot_id, secret).*
*Last verified: v0.7.11*

<sub>[◀ Connect channels](../connect-channels.md) · [Documentation index](../../README.md)</sub>
