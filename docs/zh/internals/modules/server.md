# Module: server

`src/server/`：core 外面的 axum 外壳，负责 HTTP、WebSockets、MCP、API bridge、previews serving 和 daemon assembly。所有面向网络的东西在这里，所有有状态的东西在 core。

## 职责

把 core 的 managers 暴露到 wire 上，并拥有 daemon composition：`ServerDaemon::start_background` 构建整个 runtime（stores、channel hub、input workers、plugins、search、web server、tunnel），`RunningDaemon::stop` 按顺序 unwind。

## 子模块

| Submodule | Role |
|---|---|
| `lib.rs` (`ServerDaemon`, `RunningDaemon`) | Boot sequence、64 个 sharded input workers、orphan sweep、graceful shutdown、Windows bind retry |
| `web_server/mod.rs` | Router assembly：protected vs open routes、body limits、SPA fallback |
| `web_server/api/` | 各 domain 的 REST handlers（sessions、workspaces、profiles、launcher、previews、settings、files、runtime） |
| `ws_pty` / `ws_chat` / `ws_domains` | 三类 WebSocket：terminal bytes、chat events、live-state snapshots |
| `mcp/` | 7 个 MCP tools + session identity 的 JSON-RPC dispatch |
| `api_bridge/` | 方言翻译 pipeline（[bridge 请求流程](../flows/bridge-request.md)） |
| `preview/` | Reverse proxy、iframe toolbar、markdown rendering、cookie handling |
| `auth.rs` / `pair.rs` | Token middleware（header 或 `?token=`）、local-origin rules、pairing HTTP flow |
| `bridge_recording.rs` | Launch popup 用的内存 request/response capture |
| `api_types.rs` | 和 `va-client` 共享的 wire types |

## 交互

- **→ core：** 每个 handler 都通过 core manager resolve（`ChannelManager`、`WorkspaceThreadManager`、`PtySessionManager`、`TunnelManager`、previews、profiles）。
- **← all frontends：** web SPA、desktop-ui（用到 HTTP 的地方）、TUI/CLI（通过 `va-client`）。
- **← agents：** MCP calls 和 local-api model traffic 会回流进来。
- **desktop：** 进程内嵌入 `ServerDaemon`；standalone binary 和 `va serve` 使用同一个 type。

## 不变量：不要破坏

1. **Route protection layout**：除有意开放的集合（SPA shell/assets、share previews、pairing entry）外，所有东西都 token-gated。新 routes 默认 protected；新增 open route 是安全模型变更。
2. **模型 surface 上的 local-bridge gate**：local-api / local-agent / legacy bridge routes 必须保持 loopback-only，不能被 tunnel 访问。
3. **Shutdown order matters**（`RunningDaemon::stop`）：threads → channel hub → search → `kill_all` → previews → PTYs → listeners with timeout。新子系统必须插入这个顺序，不能随手 bolt on。
4. **`ws_domains` protocol 是 snapshot-replace**：client 把最后一条消息当作当前 state；不要在这些 endpoints 引入 incremental diffs（设计上就是为了避免 schema drift）。
5. Handlers 保持 thin：parse、call core、serialize。Business rules 属于 core。

## 已知技术债

- `ws_chat.rs`（1.7k 行）混合 codec 和绕过 queue ordering 的 session-intent side effects，remediation M6。
- REST handlers + Tauri IPC + va-client + client-ts 是同一 contract 的四份手工镜像，remediation H3（desktop → HTTP，schemars type generation）。
- Server-side test density 相对 core 偏薄，remediation L11。

---

*Source anchors: `src/server/src/lib.rs`, `src/server/src/web_server/` (all submodules above), `reports/architecture-review-remediation-2026-07-04.md` (M6, H3, L11).*
*Last verified: v0.7.11*

<sub>[◀ Module: auth](auth.md) · [文档索引](../../README.md) · [Launch 子系统 ▶](../launch.md)</sub>
