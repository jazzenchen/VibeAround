# Codex 远程对比

场景是这样的：你想让 Codex 在你不在机器旁时继续干活，正在权衡几种选项 —— 托管云环境、一台 SSH 盒子，还是远程控制你自己的电脑。VibeAround 瞄准的是最后那种模式：Codex CLI 继续跑在你真实的仓库旁边，VibeAround 提供通回去的远程门。

VibeAround 是独立软件，不替代 OpenAI 的官方 Codex 产品。

## 定位

| 工作流 | 执行边界 | 最适合 |
| --- | --- | --- |
| 本地 Codex CLI + VibeAround | 用户掌控的主机 | 依赖本地文件、凭据、dev server、工具或多 Agent 工作流的工作。 |
| 托管云工作区 | 供应商管理的环境 | 能在配置好的远程环境里独立运行的任务。 |
| SSH 主机 | 用户自管的远程机器 | 以终端为中心、手动搭建的远程开发。 |
| 移动审阅界面 | 远程控制面 | 工作留在主机上时的批准、掌舵、查进度和交接。 |

## 本地主机模式长什么样

具体配置是一条十分钟的路，逐步走读见[手机上的 Codex CLI](codex-mobile.md)：

1. 安装 VibeAround，确认 Codex CLI 本地可用。
2. 连一个 IM 渠道（Telegram 最快）。
3. 通过 VibeAround 启动 Codex（`va launch --profile codex` 或桌面 Launch 页面），或给 bot 发消息开托管会话。
4. 用交接工具 + `/pickup <code>` 把终端会话交给手机；在聊天里点按权限卡片。

## VibeAround 在哪里帮上忙

- 你想让 Codex CLI 和其他 Agent 并肩 —— Claude Code、Gemini CLI、OpenCode、Cursor CLI、Qwen Code、Kiro CLI —— 同样的 Workspace、同样的远程界面。
- 你想在同一个本地会话周围有浏览器工作区、Web Terminal、IM 渠道和预览。
- 你想要供应商 Profile 或 API Bridge 路由 —— 比如让 Codex 用第三方供应商的 key 而不是订阅（[模型 Profile 指南](../guides/model-profiles.md)）。
- 你不想每个任务都被克隆进云容器。

## 实用建议

官方云端或远程产品直接契合任务时，用它。当工作流的重心是你自己的机器 —— 本地工作区状态、本地服务、私有工具、多个 Agent、供应商切换、由你掌控的远程入口 —— 时，用 VibeAround。

## 相关文档

- [手机上的 Codex CLI](codex-mobile.md)
- [远程编程](remote-coding.md)
- [支持矩阵](../product/supported-matrix.md)
- [架构总览](../architecture/overview.md)

---

*Last verified: v0.7.11*

<sub>[文档索引](../README.md)</sub>
