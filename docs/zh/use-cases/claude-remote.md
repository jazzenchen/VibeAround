# Claude Code 远程访问

场景是这样的：Claude Code 在你桌上的机器里深入一个任务，而你就要走开 —— 但会话不该停，你还想继续回答它的权限请求、读它的结果。VibeAround 的 Claude 远程访问意味着 Workspace 留在主机上，另一个界面 —— 手机、浏览器、聊天 —— 成为查看、掌舵和接续同一个会话的受控方式。

Claude 和 Claude Code 是 Anthropic 的产品。VibeAround 是协调本地工作流的独立软件。

Claude Code 也有官方的 Remote Control 能力。当工作流需要共享的本地 Agent 工作区、供应商 Profile、IM 渠道、实时预览，或想在 Claude Code、Codex CLI、Gemini CLI、OpenCode 等多个 Agent 上用同一套远程控制模式时，VibeAround 有用武之地。

## 完整走一遍

前提：VibeAround 已装好并启用了 Claude Code（[安装与上手](../guides/install-and-onboarding.md)），Claude Code 在普通终端里能正常工作。

1. **连一个 IM 渠道** —— 创建 Telegram bot，把 token 粘进桌面渠道页面（或 `settings.json` 的 `channels.telegram` + `va channel sync`）。细节见[连接渠道](../guides/connect-channels.md)。
2. **通过 VibeAround 启动 Claude Code。** 桌面 **Launch** 页面（Agent + Workspace + 模型 Profile），或从 CLI：

   ```bash
   va launch --profile claude
   ```

   你在自己的终端里得到 Claude Code 完整的原生 TUI —— VibeAround 只是渲染了环境（[Agent 启动指南](../guides/agent-launch.md)）。
3. **离开时把会话交给手机。** 在 Claude Code 会话里运行 `/vibearound handover` —— 打印一个两分钟有效的短码。在 bot 聊天里输入 `/pickup <code>`。聊天附着到同一个会话：同样的上下文、同样的 Workspace。
4. **在聊天里掌舵。** 权限请求以可点按卡片送达；`/status` 显示你附着的对象；`/new` 在同一 Workspace 干净重来。完整命令列表见 [IM 使用](../guides/im-usage.md)。
5. **回到桌前。** 启动过的会话保持可发现 —— `va launch sessions` 列出它们，桌面/控制台的恢复选择器也会提供接续。

## 供应商切换

有些团队直接用 Claude Code 的原生 Anthropic 登录。另一些用 VibeAround 的供应商 Profile 和 API Bridge 路由，让 Claude Code 跑在第三方供应商的 key 上 —— 不需要订阅编程套餐。原生路径已经稳定就用原生；需要可重复的路由、别名或 Bridge 转换时用 Profile 启动。具体步骤见 [Claude Code 供应商切换](claude-code-switcher.md)。

## 运维提示

- 加远程界面之前，先保有一套已知可用的本地 Claude Code 环境。
- 首次测试优先用私聊。
- 记清每个渠道或交接链接控制的是哪个 Workspace —— `/status` 会告诉你。
- 在重要仓库里做大范围编辑前，审阅工具动作。

## 相关文档

- [Agent 启动指南](../guides/agent-launch.md)
- [Claude Code 供应商切换](claude-code-switcher.md)
- [远程编程](remote-coding.md)
- [安全模型](../architecture/security-model.md)

---

*Last verified: v0.7.11*

<sub>[文档索引](../README.md)</sub>
