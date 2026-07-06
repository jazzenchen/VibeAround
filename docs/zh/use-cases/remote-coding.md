# 用本地 AI Agent 远程编程

场景是这样的：一个编程 Agent 正在你桌上那台机器的仓库里干活 —— 带着你的凭据、你的 dev server、你的内网 —— 而你必须离开。VibeAround 的远程编程意味着工作原地不动，你的手机或另一个浏览器变成一扇受控的、通回那台机器的门。

这和把工作搬进云端 IDE 不同。没有任何东西被克隆到供应商托管的容器里；浏览器、手机、Web Terminal、IM 和预览界面都通向同一个本地 Workspace。

## 什么时候用

- 编程 Agent 已经跑在一台受信的笔记本、台式机、工作站或服务器上。
- 项目依赖本地凭据、本地服务、内网、硬件或桌面工具。
- 你要离开键盘，但仍想审阅、批准或调整会话方向。
- 你想要远程触达，但不想把仓库克隆进云端工作区。

## 完整走一遍

VibeAround 已装好的话十五分钟；首次安装再加十分钟。

1. **安装 VibeAround** —— 从 [releases 页面](https://github.com/jazzenchen/VibeAround/releases)装桌面应用，或无界面环境 `npm i -g @vibearound/cli` 再 `va serve`（[安装与上手](../guides/install-and-onboarding.md)）。
2. **确认 Agent 本地可用。** 打开控制台（托盘 → Dashboard，或 `va status` 给的 URL），进 **Web Chat**，在你的仓库里给它一个真实任务。第一条消息自动创建 Thread（[快速导览](../guides/quick-tour.md)）。
3. **连一个 IM 渠道。** Telegram 最快：创建 bot，把 token 粘进桌面渠道页面 —— 或写进 `settings.json` 的 `channels.telegram` 再 `va channel sync`（[连接渠道](../guides/connect-channels.md)）。你的 bot 聊天现在就是完整的 Agent 对话，权限卡片可以点按。
4. **把终端会话装进口袋。** 在通过 VibeAround 启动的 Agent CLI 里运行交接工具（`/vibearound handover`）—— 得到一个两分钟有效的短码。在 bot 聊天里输入 `/pickup <code>`。同样的上下文、同样的 Workspace，从离开的地方继续。
5. **出门在外要用浏览器**，就开一条隧道。远程浏览器首次访问会先看到 6 位配对门，通过前什么都碰不到（[隧道与远程访问](../guides/tunnels-and-remote-access.md)）。

## 选你的远程界面

| 界面 | 最适合 |
| --- | --- |
| Web Chat | 浏览器里的简短指令和会话接续。 |
| Web Terminal | 对本地 Workspace 的类 shell 访问。 |
| 手机浏览器 | 离开桌子时的快速审阅、批准或调整方向。 |
| IM 渠道 | 通过 Telegram、飞书/Lark、Discord、Slack、微信、钉钉、企业微信或 QQ 机器人的异步跟进。 |
| Live Preview | 审阅本地 dev server、Markdown、HTML 和生成的产物。 |

## 安全清单

- 确认谁能触达会话 —— 渠道成员和浏览器配对就是访问边界。
- 真正需要远程访问之前，保持隧道关闭。
- 把 Web Terminal 和 IM bot 当作特权控制面；像保护 shell 一样保护它们。
- 用限定作用域的预览链接，而不是放开访问。
- 不该再接受输入的会话，及时停止或归档。

启用隧道或对外链接之前，先读[安全模型](../architecture/security-model.md)。

## 相关文档

- [会话交接](../architecture/session-lifecycle.md)
- [IM 与 Web Terminal](../guides/im-usage.md)
- [手机上的 Codex](codex-mobile.md) · [Claude Code 远程](claude-remote.md) · [Gemini CLI 远程](gemini-remote.md) · [OpenCode 远程](opencode-remote.md)

---

*Last verified: v0.7.11*

<sub>[文档索引](../README.md)</sub>
