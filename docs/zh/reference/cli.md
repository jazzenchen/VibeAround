# CLI 参考

`va` 命令（别名 `vibearound`）由 `npm i @vibearound/cli` 安装。它通过 HTTP/WS 与本地或远程守护进程通信。

全局参数：`--auth-file PATH`、`--base-url URL`、`--token TOKEN`、`--json`。

## 诊断

| 命令 | 用途 |
|---|---|
| `va help` | 显示用法 |
| `va health` | 检查公共服务存活 |
| `va info` | 显示服务元信息 |
| `va status` | 紧凑的运行时摘要 |
| `va doctor` | 诊断端点、认证和服务健康 |

## 服务与认证

| 命令 | 用途 |
|---|---|
| `va serve` | 启动独立的 VibeAround 服务 |
| `va auth status` / `va auth clear` | 显示 / 清除已保存认证 |
| `va pair start [--wait --save]` | 发起浏览器/IM 配对（可等待并保存认证） |
| `va pair status SID [--save]` / `va pair wait SID [--save]` | 轮询 / 等待配对验证 |
| `va settings reload` | 重新读取 settings.json |

## 聊天

| 命令 | 用途 |
|---|---|
| `va chat send TEXT` | 经 `/ws/chat` 发送一条提示，等待完成 |
| `va chat send --stdin` | 从标准输入读提示 |
| `va chat send --continue TEXT` | 恢复此 Workspace 已保存的聊天会话 |
| `va chat repl` | 行式聊天会话 |
| `va chat sessions` / `va chat forget [--all]` | 列出 / 遗忘本地保存的聊天会话 |

## 渠道与隧道

| 命令 | 用途 |
|---|---|
| `va channels` | 列出渠道插件运行时 |
| `va channel sync` | 把插件与 settings.json 对齐 |
| `va channel start\|stop\|restart KIND` | 插件生命周期 |
| `va tunnels` / `va tunnel kill PROVIDER` | 列出 / 停止隧道运行时 |

## Agent 与启动

| 命令 | 用途 |
|---|---|
| `va agents` | 列出已启用的 Agent |
| `va agent kill ROUTE_KEY` | 杀掉一个已附着的 Agent 运行时 |
| `va launch --profile NAME` / `--profile-path PATH` | 原生 Agent 启动（`--dry-run` 只校验） |
| `va launch sessions` | 列出可恢复的原生会话 |
| `va launch archive\|unarchive --agent A ID` | 归档 / 取消归档一个启动会话 |

## Workspace、预览、Profile

| 命令 | 用途 |
|---|---|
| `va workspaces` | 列出已注册的 Workspace |
| `va workspace add\|remove\|default PATH` | 管理 Workspace 注册表 |
| `va workspace create NAME` | 在默认根目录下创建 Workspace |
| `va previews` / `va preview delete SLUG` | 列出 / 关闭实时预览 |
| `va profiles` | 列出模型 Profile |

`va tui`（别名：`vibearound tui`、`va dashboard`、`--tui`）在同一契约上打开终端 UI。

---

*Source anchors: `src/cli/src/args.rs` (command enum and usage text — this page mirrors it 1:1).*
*Last verified: v0.7.11*

<sub>[◀ 配置参考](configuration.md) · [文档索引](../README.md) · [API 面参考 ▶](api-surfaces.md)</sub>
