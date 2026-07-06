# 连接 Telegram

配置最快的渠道：BotFather 给一个 token，写一个配置块。

## 平台侧准备

1. 在 Telegram 里打开 [@BotFather](https://t.me/botfather)。
2. 发送 `/newbot` 并按提示操作。
3. 复制 bot token。

## 配置

必填字段：`bot_token`。

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

用 `va channel sync` 使其生效（[通用模式](../connect-channels.md#通用模式)）。可选的 `verbose` 对象在每个渠道都可用：`show_thinking` 和 `show_tool_use`（默认都是 `false`）会把 Agent 的思考和工具调用块包含进聊天输出。

---

*Source anchors: [va-plugin-channel-telegram](https://github.com/jazzenchen/va-plugin-channel-telegram) `src/main.ts` (requiredConfig), `@vibearound/plugin-channel-sdk` (verbose parsing).*
*Last verified: v0.7.11*

<sub>[◀ 连接渠道](../connect-channels.md) · [文档索引](../../README.md)</sub>
