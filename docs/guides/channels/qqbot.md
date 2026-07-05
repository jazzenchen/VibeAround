# Connect QQ Bot

## Platform setup

1. Create a bot at the [QQ Open Platform](https://q.qq.com/).
2. Copy the App ID and secret.
3. Add the bot to the target guild and channels.

## Configuration

Required fields: `app_id`, `secret`.

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

> Older guides named the second field `app_token` — the plugin requires `secret`.

## Behavior

- Replies are send-only within QQ guild channels (no in-place streaming edits).

---

*Source anchors: [va-plugin-channel-qqbot](https://github.com/jazzenchen/va-plugin-channel-qqbot) `src/main.ts` (requiredConfig: app_id, secret).*
*Last verified: v0.7.11*

<sub>[◀ Connect channels](../connect-channels.md) · [Documentation index](../../README.md)</sub>
