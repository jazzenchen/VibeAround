# Web 控制台指南

控制台是守护进程在 `http://127.0.0.1:12358/va/` 提供的浏览器界面（根路径会重定向到这里）—— 终端、聊天、实时预览和运行时管理集于一个 SPA。本地从桌面应用打开时已预认证；远程则通过隧道加配对访问（[隧道与远程访问](tunnels-and-remote-access.md)）。

## Web Chat

浏览器里的完整 Agent 对话，与 IM 聊天共享同一套 Thread 模型：

- **启动选择。** 每个对话可选 Agent、Workspace（或某个 cwd）和模型 Profile；可以全新开始，也可以从会话选择器**恢复**一个原生 CLI 会话。
- **流式回合**，带工具调用进度、停止按钮和就地渲染的权限卡片。
- **斜杠命令** —— [IM 命令参考](im-usage.md)里的命令这里也能用（`/status`、`/new`、`/switch codex`……）。
- **模式与选项。** 暴露会话模式（如权限模式）或配置选项的 Agent，会把它们显示为聊天控件。
- **Warm 行为。** 回合完成或关闭标签页都不会启动 idle-shutdown 计时器。Host 保持 warm；只有以后真正的新 Host 把共用池推过[软上限](../reference/timers-and-limits.md#大小与数量)，且本 Thread 成为符合条件、最近最少活动的候选者时，才会被回收。重新打开聊天会回放近期输出；被回收的 Host 在下一条提示时透明恢复。
- **交接。** Web 对话可以被 IM 接续（`/pickup`），也可以通过移动版控制台在手机上继续。

## Web Terminal

附着在你机器上 PTY 的真实终端（xterm.js）：

- **会话**按工具创建 —— 一个 shell，或直接进入某个 Agent CLI（在终端里 `va session create --tool claude --attach` 效果相同）。
- 支持多**标签页**；会话在守护进程运行期间持续存在，关掉浏览器后可以重新附着（`va sessions` 列出它们）。
- **tmux 集成**（可选）：从控制台附着到已有的 tmux 会话；设置里的 `tmux_detach_others` 控制附着时是否踢掉其他客户端。
- 桌面版 Agent（`claude-desktop`、`codex-desktop`）无法在这里运行 —— 它们没有 CLI。

## Live Preview

不用部署，就能分享你机器上正在运行的东西：

- **Dev server 预览。** 注册一个本地端口，得到一个反向代理它的预览页，带 iframe 工具栏。Agent 启动 dev server 时会自动创建这些预览（通过 `va-preview` 技能 / MCP `preview` 工具 —— [工具参考](../reference/api-surfaces.md#mcp-tools)）。
- **Markdown 预览。** 任何 markdown 文件按 GitHub 风格渲染（`md_preview` 工具或 `va-md-preview` 技能）。
- **每个预览有两条链接：** owner URL（token 认证，与预览同寿命）和 share URL（10 分钟过期、无需认证 —— 可以放心贴到群聊里）。见[安全模型](../architecture/security-model.md)。
- `va previews` / `va preview delete <slug>` 从 CLI 管理它们；为你代启的预览进程会随守护进程停止而被杀掉。

## 运行时管理

控制台面板与 `va status` 汇报的内容一一对应 —— 渠道插件状态（带重启控制）、隧道状态、活跃的 Agent 运行时、PTY 会话、Workspace 和模型 Profile。面板上能做的事都有对应的 CLI 命令（[配置参考](../reference/configuration.md)）。

## 移动端

控制台是响应式的；聊天界面包含移动端命令控件，Thread 命令都可以点按。配对一次（60 秒有效的配对码，在受信界面上确认），之后手机上打开隧道化的控制台就和桌面上一样。

---

*Source anchors: `src/server/src/web_server/` (ws_chat, ws_pty, preview/), `src/web/src/` (SPA), `src/core/src/pty/` (sessions), `src/core/src/previews/` (owner/share, TTL), `src/skills/va-preview/`, `src/skills/va-md-preview/`.*
*Last verified: v0.7.11*

<sub>[◀ 桌面应用指南](desktop-app.md) · [文档索引](../README.md) · [IM 使用 ▶](im-usage.md)</sub>
