# 连接微信

微信通过 OpenClaw 兼容桥接接入，扫码登录 —— 设置里不放任何凭据。

## 准备

1. 添加下面的配置块（除了 `verbose` 可以是空的）并启动 VibeAround。
2. 插件会在终端或桌面应用里显示二维码。
3. 用微信手机 App 扫码。

## 配置

必填字段：无（认证在运行时通过扫码完成）。

```json
{
  "channels": {
    "weixin-openclaw-bridge": {
      "verbose": { "show_thinking": false, "show_tool_use": false }
    }
  }
}
```

注意 kind id 是 `weixin-openclaw-bridge`，不是 `wechat`。

## 行为

- 回复是纯文本、只发送模式 —— 没有流式编辑，没有交互卡片（权限请求退化为文本）。

---

*Source anchors: [va-plugin-channel-weixin-openclaw-bridge](https://github.com/jazzenchen/va-plugin-channel-weixin-openclaw-bridge) (QR runtime auth).*
*Last verified: v0.7.11*

<sub>[◀ 连接渠道](../connect-channels.md) · [文档索引](../../README.md)</sub>
