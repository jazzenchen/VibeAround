# 手机上的 Codex CLI

场景是这样的：Codex CLI 在你桌上的机器里做一个长时间重构做到一半，你要去吃午饭、开会或通勤。有了 VibeAround，会话继续跑在本地主机上，你的手机变成审阅结果、回答 Agent 提问、推动任务前进的地方。

VibeAround 是独立软件，与 OpenAI 无关。Codex 和 ChatGPT 是 OpenAI 的产品。如果你在比较托管 Codex 工作流、SSH、隧道和 VibeAround 的本地主机方案，读完本文后见 [Codex 远程对比](codex-remote.md)。

## 完整走一遍

前提：VibeAround 已装好并启用了 Codex（[安装与上手](../guides/install-and-onboarding.md)），Codex CLI 在普通终端里能正常工作。

1. **一次性连好 IM 渠道。** Telegram 最快：创建 bot，把 token 粘进桌面渠道页面（或 `settings.json` 的 `channels.telegram` + `va channel sync`）。细节见[连接渠道](../guides/connect-channels.md)。
2. **在桌前开始干活。** 要么通过 VibeAround 在自己终端里启动 Codex（桌面 **Launch** 页面：Agent + Workspace + Profile，或 `va launch --profile codex`），要么直接给 bot 发消息 —— 第一条消息就会在 Workspace 里拉起托管的 Codex 会话。
3. **把终端会话交接给手机。** 在启动的 CLI 里运行交接工具（`/vibearound handover`）。你得到一个两分钟有效的短码。在 bot 聊天里：

   ```text
   /pickup K7PQ
   ```

   聊天附着到终端会话上 —— 同样的上下文、同样的 Workspace。
4. **在手机上掌舵。** 权限请求以可点按卡片送达。离开期间有用的命令：

   ```text
   /status        我附着在什么上面？忙还是闲？
   /session       列出此 Workspace 可恢复的会话
   /new           放弃当前方向，开新 Thread
   ```

5. **用 [Live Preview](../guides/web-dashboard.md) 直观审阅输出**：已配对的 owner 链接和由访问码保护的 Server/Markdown Share 都可以在手机打开。Server Share 会原样转发已认证的 GET/HEAD 路径，包括页面的数据读取；写请求、协议升级、service worker、WebSocket 与 HMR 暂不支持，`/va/*`、owner 页面、chat 和审阅控件不进入 Share。

## 手机擅长什么

手机适合掌舵、批准和审阅 —— 不适合大范围翻文件。

- 问 Agent 进度，批准或否决一个提议的方向。
- 要求跑一个聚焦的测试或构建，读摘要。
- 审阅预览链接。
- 暂停、归档，或把会话交还给桌面。

## 安全提示

把手机当作本地 Workspace 的控制面。像保护直接 shell 访问一样保护浏览器配对、IM 渠道成员和终端访问 —— 见[安全模型](../architecture/security-model.md)。

## 相关文档

- [Codex 远程对比](codex-remote.md)
- [远程编程](remote-coding.md)
- [IM 使用 —— 完整命令参考](../guides/im-usage.md)
- [模型 Profile 指南](../guides/model-profiles.md)

---

*Last verified: v0.7.11*

<sub>[文档索引](../README.md)</sub>
