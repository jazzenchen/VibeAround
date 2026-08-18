# Agent 启动指南

Agent Launch 在终端里打开编程 Agent CLI，并在启动前准备凭据、模型路由、项目级 skills 和 MCP config。机制见[启动子系统内幕](../internals/launch.md)。

## 从桌面应用启动

Launch 页面需要三个选择：

1. **Agent** —— 任何已启用的 Agent，包括桌面版目标（`claude-desktop`、`codex-desktop`），它们打开厂商的 GUI 应用而不是 CLI。
2. **Workspace** —— Agent 的起始目录。
3. **模型 Profile** —— `direct`（Agent 自己的官方登录）或你的某个供应商 Profile（见[模型 Profile 指南](model-profiles.md)）。

VibeAround 渲染这次启动 —— 环境变量、按 Agent 的配置覆盖、需要时的 Bridge URL —— 然后打开你的终端应用（macOS 上是 Terminal.app 或 iTerm2，Windows 上是 PowerShell，Linux 上是 `xdg-terminal-exec`/常见终端）。终端偏好可配置。

## 从 CLI 启动

保存的启动配置在 `~/.vibearound/launch/profiles/<name>.json`：

```bash
va launch --profile codex           # 按名称
va launch --profile-path ./my.json  # 按文件
va launch --profile codex --dry-run # 校验并打印计划，不实际启动
```

`va launch` 实际执行的是捆绑的原生 `va-launch` 二进制 —— 一个可独立使用的启动器（`/path/to/va-launch --profile <name>`），脚本化和类 CI 的启动因此不需要完整 CLI。

启动配置是一个小 JSON 文档（schema 版本 1）：Agent、Workspace、终端选择、命令/可执行文件覆盖、环境变量、参数、窗口标签。未知字段会被拒绝，以防递错 JSON。

## 启动时发生什么

1. **校验。** Workspace 存在、Agent 可执行文件可解析（显式路径 → `~/.vibearound/agents.json` → PATH 扫描，首次发现后缓存）。
2. **Workspace 准备。** 每次启动都会删除已知的旧 VibeAround skill 名称，并写入当前 bundled 项目级 skills。若 `auth-mcp.json` 可用，同时把当前 daemon-scoped MCP credential 写入 Agent 的项目配置；credential 缺失时只跳过 MCP 配置，不会跳过 skill sync，也不会删除项目文件。
3. **终端拉起。** Agent 带着渲染好的环境在你的终端里启动。Bridge 化的 Profile 下，它的模型流量经过 `127.0.0.1:12358` —— 所以会话存续期间保持守护进程运行。

## 启动产生的会话

启动的 CLI 会创建自己的原生会话，VibeAround 能发现它们：

```bash
va launch sessions                     # 跨 Agent/Workspace 的可恢复会话
va launch archive --agent claude <id>  # 从选择器中隐藏某一个
va launch unarchive --agent claude <id>
```

这些会话出现在桌面/控制台的恢复选择器里，可以用 `/session --switch <id>` 附着到聊天，或用 CLI 内的交接工具 + `/pickup <code>` 交接出去。这就是"在终端里干活"和"在手机上继续"之间的桥 —— 见[会话生命周期](../architecture/session-lifecycle.md)。

## Launch 还是托管：怎么选

| | Launch（你的终端） | 托管（IM / Web Chat） |
|---|---|---|
| UI | Agent 自己的完整 TUI | 聊天气泡 + 权限卡片 |
| 进程属主 | 你的终端 | 守护进程（由 warm pool 管理） |
| 模型路由 | Profile 渲染的配置 | 同样的 Profile、同样的 Bridge |
| 连续性 | 会话可被发现，交接靠交接码 | 会话自动跟随 Thread |
| 守护进程停止后 | CLI 继续运行；Bridge 化的模型调用失败，直到守护进程回来 | 不存续（Agent 由守护进程托管） |

## 故障排查

| 症状 | 处理 |
|---|---|
| "executable not found" | 安装该 Agent CLI，或在启动配置里设显式 `executablePath`；删掉 `~/.vibearound/agents.json` 里的过期条目强制重新扫描 |
| 终端一打开就关闭 | 用 `--dry-run` 看计划；把它打印的命令手动跑一遍，暴露真实错误 |
| Agent 启动了但模型调用失败 | 守护进程没在运行（Bridge 化 Profile），或 Profile key 无效 —— 查 `va status` 和 Profile |
| Linux：什么都没打开 | 没找到受支持的终端；装一个常见终端或设置终端偏好 |

---

*Source anchors: `src/launcher/` (va-launch: validation, integrations, spawn), `~/.vibearound/launch/profiles/` schema (launch profile JSON v1), `src/core/src/agent/launch.rs` + `src/core/src/profiles/bridge_launch.rs` (profile rendering), `src/core/src/launch_sessions/` (session discovery), `src/cli/src/args.rs` (va launch commands).*
*Last verified: v0.7.11*

<sub>[◀ 模型 Profile 指南](model-profiles.md) · [文档索引](../README.md) · [隧道与远程访问 ▶](tunnels-and-remote-access.md)</sub>
