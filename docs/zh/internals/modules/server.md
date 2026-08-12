# Module: server

`src/server/`：core 外面的 axum 外壳，负责 HTTP、WebSockets、MCP、API bridge、previews serving 和 daemon assembly。所有面向网络的东西在这里，所有有状态的东西在 core。

## 职责

把 core 的 managers 暴露到 wire 上，并拥有 daemon composition：`ServerDaemon::start_background` 构建整个 runtime（stores、channel hub、input workers、plugins、search、web server、tunnel），`RunningDaemon::stop` 按顺序 unwind。

## 子模块

| Submodule | Role |
|---|---|
| `lib.rs` (`ServerDaemon`, `RunningDaemon`) | Boot sequence、channel input dispatcher、orphan sweep、ingress-first shutdown、Windows bind retry |
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

1. **Route protection layout**：除有意开放的集合（SPA shell/assets、由访问码保护的 Preview Share、pairing entry）外，所有东西都 token-gated。Owner Preview routes 要求 loopback/token 访问或远程配对。Server Share 只接受 GET/HEAD iframe 导航和浏览器通过 `Sec-Fetch-Dest` 声明的静态子资源，并拒绝非 GET/HEAD、fetch/XHR/EventSource、worker、WebSocket 与 HMR。它不是通用 API 兼容层或 API 隔离沙盒；`/va/*`、owner、chat 与 review 不进入 Share。新 routes 默认 protected；新增 open route 是安全模型变更。
2. **模型 surface 上的 local-bridge gate**：local-api / local-agent / legacy bridge routes 必须保持 loopback-only，不能被 tunnel 访问。
3. **Shutdown order matters**：先停 Web/input ingress，再 drain `ConversationIngress`，然后停止 channel、workspace hosts、search，最后 safety-net registry kill、previews、PTY 和 listeners。teardown 后不能再执行排队 prompt。
4. **`ws_domains` protocol 是 snapshot-replace**：client 把最后一条消息当作当前 state；不要在这些 endpoints 引入 incremental diffs（设计上就是为了避免 schema drift）。
5. Handlers 保持 thin：parse、call core、serialize。Business rules 属于 core。

## 已知技术债

- `ws_chat.rs` 已拆 parser/event，但 session-intent side effects 仍早于 route lane，可在多 WebSocket 下交错。
- REST handlers + Tauri IPC + va-client + client-ts 仍是同一 control-plane contract 的手工镜像。
- Unit test 较广，但 cross-surface contract 与 lifecycle fault integration 仍偏薄。

---

*Source anchors: `src/server/src/lib.rs`, `src/server/src/web_server/`。*
*Last verified: `codex/im-acp-route-refactor` at `0ba7fa2e`（2026-07-11）。*

<sub>[◀ Module: auth](auth.md) · [文档索引](../../README.md) · [Launch 子系统 ▶](../launch.md)</sub>
