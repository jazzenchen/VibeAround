# Gemini CLI 远程访问

场景是这样的：Gemini CLI 在你桌上的机器里排查一个 bug 或起草改动，你想让这个会话在手机或另一个浏览器上保持可达 —— 又不想把项目搬进托管环境。VibeAround 让会话继续跑在本地 Workspace 旁边，并给你受控的远程门通回去。

Gemini 是 Google 的产品。VibeAround 是协调本地 Agent 工作流的独立软件。

## 完整走一遍

1. **确认本地基线。** 在 VibeAround 之外安装并认证 Gemini CLI，确认它能在目标仓库里工作。先有一套可用的本地环境，后面每一步都好排查。
2. **在 VibeAround 里启用 Gemini** —— 引导期间或从桌面 Agent 页面（[安装与上手](../guides/install-and-onboarding.md)）。
3. **启动或接续会话。** 桌面 **Launch** 页面（Gemini + Workspace + Profile）或 `va launch --profile <name>` 在自己终端里启动；或给已连接的渠道 / Web Chat 发消息，拉起托管会话（[Agent 启动指南](../guides/agent-launch.md)）。
4. **加上远程界面。** 连一个 IM 渠道（[连接渠道](../guides/connect-channels.md)），然后在 CLI 里 `/vibearound handover`、在聊天里 `/pickup <code>` 把终端会话移过去 —— 或者直接在同一个聊天里继续指挥托管会话。
5. **审阅输出** —— 用 [Live Preview](../guides/web-dashboard.md) 在手机打开已配对的 owner 链接，或由访问码保护的 Server/Markdown Share。Server Share 会原样转发已认证的 GET/HEAD 路径，包括页面的数据读取；写请求、协议升级、service worker、WebSocket 与 HMR 暂不支持，`/va/*`、owner 页面、chat 和审阅控件不进入 Share。

## 常见用法

- 另一个 Agent 在编辑时，找 Gemini CLI 要第二意见 —— `/agent --switch <id>` 在已启用的 Agent 之间移动 Thread。
- 本地主机在线时，从手机继续一项调查。
- 工作流需要显式选择端点或模型时，用供应商 Profile（[模型 Profile 指南](../guides/model-profiles.md)）。
- 不掏笔记本就审阅一个生成的预览。

## 故障排查

Gemini CLI 经 VibeAround 不工作时，先在同一 Workspace 直接跑它。然后检查所选的终端模式、Profile 路由、环境变量和本地认证状态 —— 完整清单见[故障排查指南](../guides/troubleshooting-and-faq.md)。

## 相关文档

- [支持矩阵](../product/supported-matrix.md)
- [模型 Profile 指南](../guides/model-profiles.md)
- [IM 使用 —— 完整命令参考](../guides/im-usage.md)
- [远程编程](remote-coding.md)

---

*Last verified: v0.7.11*

<sub>[文档索引](../README.md)</sub>
