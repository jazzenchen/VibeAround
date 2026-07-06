# 连接 WhatsApp

WhatsApp 通过 Baileys（非官方 WhatsApp Web 客户端）接入，首次运行扫码登录；之后会话持久化到磁盘。

## 准备

1. 添加下面的配置块并启动 VibeAround。
2. 用 WhatsApp 手机 App（Linked devices）扫描首次运行显示的二维码。

## 配置

必填字段：无（运行时扫码认证）。

```json
{
  "channels": {
    "whatsapp": {
      "verbose": { "show_thinking": false, "show_tool_use": false }
    }
  }
}
```

## 行为

- 非官方客户端注意事项：WhatsApp 没有为这种用法提供官方 bot API；把这条连接当作尽力而为，并保持 Linked devices 列表整洁。

---

*Source anchors: [va-plugin-channel-whatsapp](https://github.com/jazzenchen/va-plugin-channel-whatsapp) `src/main.ts` (QR auth, session persisted).*
*Last verified: v0.7.11*

<sub>[◀ 连接渠道](../connect-channels.md) · [文档索引](../../README.md)</sub>
