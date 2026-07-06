# Connect WeChat

WeChat connects through an OpenClaw-compatible bridge with QR login — no credentials in settings.

## Setup

1. Add the config block below (it can be empty apart from `verbose`) and start VibeAround.
2. The plugin shows a QR code in the terminal or desktop app.
3. Scan it with the WeChat mobile app.

## Configuration

Required fields: none (authentication happens at runtime via QR).

```json
{
  "channels": {
    "weixin-openclaw-bridge": {
      "verbose": { "show_thinking": false, "show_tool_use": false }
    }
  }
}
```

Note the kind id is `weixin-openclaw-bridge`, not `wechat`.

## Behavior

- Replies are plain text and send-only — no streaming edits, no interactive cards (permission requests degrade to text).

---

*Source anchors: [va-plugin-channel-weixin-openclaw-bridge](https://github.com/jazzenchen/va-plugin-channel-weixin-openclaw-bridge) (QR runtime auth).*
*Last verified: v0.7.11*

<sub>[◀ Connect channels](../connect-channels.md) · [Documentation index](../../README.md)</sub>
