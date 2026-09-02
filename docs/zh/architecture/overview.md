# 工作原理

本页跟随定义 VibeAround 的两段旅程：一条 IM 消息抵达编程 Agent，和一个 Agent CLI 在你的终端里被启动。术语不熟悉的话，先看[核心概念](concepts.md)。

## 运行时一览

```text
 web SPA ───────────────┐       TUI / CLI (va)          desktop-ui (React)
  │HTTP   │WS ×2        │             │ HTTP + WS            │ Tauri IPC
  │/api/* │/ws/chat     │             │ (va-client)          ▼
  │       │/ws/* state  │             │                desktop (Tauri shell)
  │       │             │             │                      │ 进程内嵌入
  ▼       ▼             ▼             ▼                      ▼
   ┌───────────────────────────────────────────────────────────────┐
   │              vibearound-server (axum daemon)                  │
   │  REST /api/* · WS (chat, live state) · MCP /mcp               │
   │  api_bridge /va/local-api · preview 反向代理 · 配对           │
   ├───────────────────────────────────────────────────────────────┤
   │  core 运行时                                                  │
   │  channels · workspace threads · process supervisor ·          │
   │  profiles · previews · tunnels · auth · search                │
   └──┬──────────────┬──────────────────────────────┬──────────────┘
      │ stdio        │ stdio                        │ 子进程
      │ JSON-RPC     │ JSON-RPC                     ▼
      │ (ACP)        │ (ACP)                    隧道进程
      ▼              ▼                          (cloudflared, ngrok,
  渠道插件进程    agent CLI                     npx localtunnel,
  (telegram, …)   进程                          tailscale funnel)
                     │
                     │ HTTP 回环（Bridge 化的 Profile）
                     ▼
             /va/local-api bridge ──HTTPS──► 上游模型 API
```

一切都是一个进程加受监督的子进程。桌面应用内嵌的就是独立 `va serve` 跑的那个守护进程；每个 UI 都是它的客户端。

## 通信路径

图中每一条边，及其传输方式和负载形状：

| 边 | 传输 | 协议 / 负载 |
|---|---|---|
| web SPA → server（REST） | HTTP `/api/*` | JSON，bearer token |
| web SPA ↔ server（聊天） | WebSocket `/ws/chat` | JSON 聊天事件：类型化输入、流式输出、权限卡片 |
| web SPA ↔ server（实时状态） | WebSocket `/ws/channels`、`/ws/tunnels`、`/ws/agents/runtime` | 每次变化重发全量快照（"最后一条消息就是状态"） |
| TUI / `va` CLI → server | HTTP + `/ws/chat` | 同一套契约，经 `va-client` 协议 crate |
| desktop-ui → 桌面外壳 | Tauri IPC | 原生命令（窗口、启动、配置页面） |
| 桌面外壳 → 守护进程 | 进程内 | 直接嵌入 `ServerDaemon` |
| 守护进程 ↔ 渠道插件 | 子进程 stdio | 换行分隔的 JSON-RPC（ACP 帧）：信封进；输出、权限卡片、`_va/heartbeat` 出 |
| 守护进程 ↔ agent CLI | 子进程 stdio | ACP（JSON-RPC）：初始化、会话、提示、通知、权限请求 |
| agents → 守护进程（工具） | HTTP `/mcp` | MCP：streamable HTTP 上的 JSON-RPC（+ SSE） |
| 启动的 CLI → 守护进程（模型） | HTTP 回环 `/va/local-api/…` | 客户端的供应商方言（OpenAI / Anthropic / Gemini 形状） |
| 守护进程 → 模型供应商 | HTTPS | Bridge 转换后的供应商方言 |
| 守护进程 → 隧道 | 受监管子进程（`cloudflared`、`npx localtunnel`、`ngrok http`、`tailscale funnel`） | 供应商特定 |
| 守护进程 → 被预览的 dev server | HTTP 反向代理 | 透传 + iframe 工具栏注入 |

## 模块地图

每项职责的归属。每个运行时模块在 [modules/](../internals/README.md#modules) 下都有深入页面，每条端到端路径在 [flows/](../internals/README.md#flows) 下都有走读：

**`core` —— 运行时库（没有 HTTP 服务，没有 UI）：**

| 模块 | 负责 |
|---|---|
| `channels` | 插件宿主、stdio/websocket 传输、输入分发、route lanes、监控 |
| `workspace` | Workspace、Thread、Route 附着、交接码（事件溯源状态 + 内存 pickup codes） |
| `process` | 监督器（拉起/重启/看门狗，持有全部子进程）、孤儿清扫、ACP 传输、环境增强 |
| `agent` | ACP agent 句柄、启动渲染、MCP/技能配置注入 |
| `profiles` | Profile schema、目录、渲染、Bridge 启动 URL、供应商连接 |
| `previews` | Server/Markdown owner 与 Share URL、受限 Server Share 代理、端口清理 |
| `tunnels` | ngrok / localtunnel / cloudflare / Tailscale Funnel 各供应商 |
| `auth` | 守护进程 token、配对码 |
| `launch_sessions` | 原生 CLI 会话发现与归档 |
| `plugins` | 插件发现与清单 |
| `search` | 宿主侧网页搜索运行时 |
| `config`、`storage`、`state`、`routing`、`resources` | 设置、JSONL 事件存储、StateSource 契约、route key、Agent 注册表 |

**`server` —— core 之上的 axum 外壳：** `web_server/api`（REST）、`ws_chat` / `ws_domains`（两族 WebSocket）、`mcp`（工具端点）、`api_bridge`（方言转换、模型映射、内容策略、上游）、`preview`（反向代理、markdown 渲染）、`auth` + `pair`（token 中间件、配对）、`boot`（守护进程装配）。

**其他 crate：** `client`（HTTP/WS 契约的纯 Rust 协议库）、`cli` 和 `tui`（它的消费者）、`desktop`（Tauri 外壳 + IPC 命令）、`launcher`（`va-launch` 原生启动二进制）。

## 旅程一：一条 IM 消息变成 Agent 回复

1. **平台到插件。** Telegram/飞书/Slack 插件 —— 由守护进程监督的独立 Node.js 进程 —— 接收平台的 webhook 或长轮询事件，归一化为渠道信封：route key、消息 id、文本、附件。

2. **插件到守护进程。** 信封经插件的 stdio ACP 连接进入守护进程，落到渠道输入队列。

3. **有序分发。** 输入按 route key 分片到 worker 任务：同一聊天的消息严格按序处理，不同聊天并行。这就是连珠炮消息不会互相竞态的原因。

4. **命令还是提示。** 文本先对照斜杠命令语法检查（`/new`、`/close`、`/switch`、`/pickup`……）。命令由 workspace-thread 层直接处理；其余都成为提示。

5. **Route 到 Thread。** Route 解析到它附着的开启 Thread。某 Route 首次联系会在默认 Workspace 创建 Thread 并附着 —— 不需要任何设置步骤。

6. **Thread 到 Agent。** Thread 确保宿主 Agent 活着：需要时监督器在 Workspace 目录里拉起该 Agent 的 ACP 适配器，绑定 Profile 的环境已就位（凭据、Bridge base URL），VibeAround 的 MCP 端点和技能注入 Agent 配置。Thread 已有 CLI Session 就恢复它；否则新建一个。

7. **提示与流式回传。** 提示经 ACP 发给 Agent。通知流回来 —— 文本块、工具调用摘要、权限请求 —— 并扇出到附着在该 Thread 上的每条 Route。权限请求渲染为交互卡片；点按以回调返回，解除 Agent 的等待。

8. **保持 warm，压力下回收。** 回合结束后 Host 继续常驻，便于快速复用；Web 和 IM 都不会启动固定的闲置关停计时器。真正的新 Host 启动后，如果 warm pool 超过软上限，最多回收一个符合条件、最近最少活动的 Host。Thread runtime 与 CLI Session 保持完整，被回收的 Host 会在下一条消息时恢复。见[计时器与上限](../reference/timers-and-limits.md#大小与数量)。

## 旅程二：在你的终端里启动 Agent CLI

Agent Launch 是另一条路 —— 不把 Agent 托管在守护进程里，而是在你自己的终端里打开它：

1. 你选 Agent、Workspace 和模型 Profile（桌面 UI 或 `va launch --profile <name>`）。
2. Profile 被渲染成一次具体启动：环境变量、按 Agent 的配置覆盖，以及 —— 当 Profile 指向 Bridge 化的供应商时 —— 指向守护进程本地 API 的 base URL（`http://127.0.0.1:12358/va/local-api/…`）。
3. 原生启动器（`va-launch`，随桌面应用和 CLI 一起发行的独立二进制）校验计划，守护进程在运行时安装项目级 MCP/技能集成，然后在你的终端应用里打开 Agent（Terminal.app、iTerm2、PowerShell 或某个 Linux 终端）。
4. 启动的 CLI 的模型请求走 Bridge，所以一份 Kimi 或 DeepSeek 订阅可以原生驱动 Codex 或 Claude Code。这样创建的会话会被守护进程发现并显示为可恢复 —— 这就是终端会话后来能从 IM 接续的原理。

## 状态放在哪

| 状态 | 位置 | 重启后保留？ |
|---|---|---|
| Workspace、Thread、Route 附着 | `~/.vibearound/` 里的 JSONL 事件日志 | 保留 |
| Agent CLI 会话（对话记录） | 各 Agent 自己的存储 | 保留（归 Agent 所有） |
| 模型 Profile、设置 | `~/.vibearound/settings.json` + Profile 存储 | 保留 |
| 运行中的 Agent/插件进程 | 内存中，受监督 | 不保留 —— 按需重新拉起 |
| 控制台认证 token | `~/.vibearound/auth.json` | 每次守护进程启动重新生成 |

监督器给每个子进程（渠道插件、Agent 适配器）提供崩溃重启，插件另有心跳看门狗；守护进程关停时级联清理，不留孤儿进程。

---

*Source anchors: `src/server/src/lib.rs` (daemon boot, input sharding), `src/server/src/web_server/mod.rs` + `ws_chat.rs` / `ws_domains.rs` (WebSocket families), `src/core/src/lib.rs` (module map), `src/core/src/channels/` (plugin transport, dispatch), `src/core/src/workspace/` (threads, attachments), `src/core/src/process/supervisor.rs` + `acp_transport.rs` (ACP framing), `src/core/src/tunnels/providers/` (tunnel process forms), `src/core/src/profiles/bridge_launch.rs` (local API URLs), `src/launcher/` (va-launch).*
*Last verified: v0.7.11*

<sub>[◀ 核心概念](concepts.md) · [文档索引](../README.md) · [会话生命周期 ▶](session-lifecycle.md)</sub>
