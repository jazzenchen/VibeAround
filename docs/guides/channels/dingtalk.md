# Connect DingTalk

DingTalk uses Stream Mode (WebSocket) — no public callback URL needed.

## Platform setup

1. Create an app at the [DingTalk Open Platform](https://open.dingtalk.com/).
2. Enable **Stream Mode** in the app settings.
3. Copy the Client ID and Client Secret.
4. Grant message-event permissions in the app permission list.

## Configuration

Required fields: `client_id`, `client_secret`.

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

> Older guides named these fields `app_key` / `app_secret` — the plugin requires `client_id` / `client_secret`.

## Behavior

- Nothing arrives? Stream Mode not enabled is the usual cause.

---

*Source anchors: [va-plugin-channel-dingtalk](https://github.com/jazzenchen/va-plugin-channel-dingtalk) `src/main.ts` (requiredConfig: client_id, client_secret).*
*Last verified: v0.7.11*

<sub>[◀ Connect channels](../connect-channels.md) · [Documentation index](../../README.md)</sub>
