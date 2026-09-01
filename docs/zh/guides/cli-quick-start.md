# CLI 快速开始

当你需要无界面的 VibeAround 守护进程、终端优先的工作流，或在没有桌面应用的远程机器上运行时，使用 npm CLI。CLI 启动的是同一个服务，也使用和桌面版相同的 `~/.vibearound/` 数据目录。

## 1. 安装 CLI

```bash
npm i -g @vibearound/cli
va --help
```

如果你已经在运行桌面应用，也可以安装 CLI；桌面应用运行时，CLI 会连接同一个本地守护进程。

## 2. 启动守护进程

```bash
va serve
```

守护进程绑定 `127.0.0.1:12358`，首次运行会创建 `~/.vibearound/settings.json`，并把新的本地认证 token 写入 `~/.vibearound/auth.json`。

另开一个终端：

```bash
va status
va doctor
```

`va status` 会打印控制台 URL、运行时状态、渠道、隧道和活跃会话。`va doctor` 会检查认证、端点可达性和服务健康。

## 3. 打开 TUI 或浏览器控制台

```bash
va tui
```

TUI 是终端里最快的运行时查看入口。浏览器控制台则使用 `va status` 给出的已认证 URL；它提供和桌面应用相同的 Web Chat、实时预览、Workspace 列表、Profile 和运行时控制。

## 4. 添加 Workspace 和 Agent

```bash
va workspace add ~/dev/my-app
va workspaces
va agents
```

Agent 仍然需要先按自己的原生方式安装和登录：Claude Code、Codex CLI、Gemini CLI、OpenCode 等工具，应当先能在普通终端里正常工作，再交给 VibeAround 托管或启动。

## 5. 启动或发送消息

在自己的终端里启动一个原生 Agent CLI：

```bash
va launch --profile my-codex-profile
```

或者发送一条托管 Web Chat 提示：

```bash
va chat send "look at ~/dev/my-app and summarize the project"
```

长时间交互更适合用 Web Chat、IM 渠道或 TUI。完整命令见 [CLI 参考](../reference/cli.md)。

---

*Source anchors: `src/cli/src/args.rs` (CLI command surface), `src/server/src/lib.rs` (daemon), `src/core/src/config.rs` (settings/auth paths).*
*Last verified: v0.7.12*

<sub>[◀ 快速导览](quick-tour.md) · [文档索引](../README.md) · [桌面应用指南 ▶](desktop-app.md)</sub>
