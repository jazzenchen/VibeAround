# OpenCode 远程访问

场景是这样的：OpenCode 跑在主机上、仓库和工具旁边，你想在休息时用手机看看它 —— 或干脆把会话整个交接过去，窝在沙发上继续。VibeAround 让执行留在你自己的电脑上，提供浏览器、手机、终端、IM 和预览入口通回这个会话。

## 完整走一遍

1. **确认 OpenCode 在目标仓库的本地终端里能跑。** 认证和配置问题先在那里解决。
2. **在 VibeAround 里启用 OpenCode**（[安装与上手](../guides/install-and-onboarding.md)），把项目目录加为 Workspace。
3. **启动或附着会话。** 桌面 **Launch** 页面或 `va launch --profile <name>` 在自己终端里启动；或给已连接的渠道 / Web Chat 发消息开托管会话（[Agent 启动指南](../guides/agent-launch.md)）。
4. **从另一台设备继续。** 在 CLI 会话里运行 `/vibearound handover`，然后在 bot 聊天里 `/pickup <code>` —— 或从已配对的浏览器打开 Web Chat。权限请求会以可点按卡片跟着你走。
5. **等直连的本地会话稳定之后再加 IM** —— 这样之后的每个问题都能归因到单独一层。

## 适合的场合

- 想用一套 Workspace 模型容纳多个编程 Agent 的团队 —— OpenCode 与 Claude Code、Codex CLI、Gemini CLI 等并肩。
- 想在手机或浏览器终端里够到 OpenCode 的开发者。
- 需要本地 dev server、本地包缓存或内网访问的工作流。
- 受益于生成的网页/Markdown/HTML 预览链接的会话（[Live Preview](../guides/web-dashboard.md)）。

## 需要验证的限制

支持程度可能随 OpenCode 版本、终端模式、会话持久化和 Profile 路由而变 —— 当前状态记录在[支持矩阵](../product/supported-matrix.md)。在重要仓库上使用之前，先用小任务验证启动、接续和供应商路由。

## 相关文档

- [支持矩阵](../product/supported-matrix.md)
- [远程编程](remote-coding.md)
- [IM 使用 —— 完整命令参考](../guides/im-usage.md)
- [故障排查](../guides/troubleshooting-and-faq.md)

---

*Last verified: v0.7.11*

<sub>[文档索引](../README.md)</sub>
