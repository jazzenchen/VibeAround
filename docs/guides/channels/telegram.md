# Connect Telegram

The fastest channel to set up: one token from BotFather, one config block.

## Platform setup

1. Open [@BotFather](https://t.me/botfather) in Telegram.
2. Send `/newbot` and follow the prompts.
3. Copy the bot token.

## Configuration

Required fields: `bot_token`.

```json
{
  "channels": {
    "telegram": {
      "bot_token": "123456789:ABCdefGHIjklMNOpqrSTUvwxYZ",
      "verbose": { "show_thinking": true, "show_tool_use": true }
    }
  }
}
```

Apply with `va channel sync` ([the pattern](../connect-channels.md#the-pattern)). The optional `verbose` object works on every channel: `show_thinking` and `show_tool_use` (both default `false`) include the agent's thinking and tool-use blocks in chat output.

---

*Source anchors: [va-plugin-channel-telegram](https://github.com/jazzenchen/va-plugin-channel-telegram) `src/main.ts` (requiredConfig), `@vibearound/plugin-channel-sdk` (verbose parsing).*
*Last verified: v0.7.11*

<sub>[◀ Connect channels](../connect-channels.md) · [Documentation index](../../README.md)</sub>
