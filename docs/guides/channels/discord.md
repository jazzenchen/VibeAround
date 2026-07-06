# Connect Discord

Bot token plus two portal switches; the bot answers where it is @mentioned.

## Platform setup

1. Create an application in the [Discord Developer Portal](https://discord.com/developers/applications) and generate a token under **Bot**.
2. Enable Privileged Gateway Intents: **Message Content Intent** ON (required); Server Members Intent optional.
3. Under **OAuth2 → URL Generator**, select scope `bot` with permissions `Send Messages`, `Read Message History`, `Embed Links`.
4. Open the generated invite URL to add the bot to your server.

## Configuration

Required fields: `bot_token`.

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

## Behavior

- In server channels the bot only responds when **@mentioned**; each channel where it is mentioned becomes its own conversation route.
- No incoming messages at all? Message Content Intent is the usual culprit.

---

*Source anchors: [va-plugin-channel-discord](https://github.com/jazzenchen/va-plugin-channel-discord) `src/main.ts` (requiredConfig).*
*Last verified: v0.7.11*

<sub>[◀ Connect channels](../connect-channels.md) · [Documentation index](../../README.md)</sub>
