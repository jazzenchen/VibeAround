# Module: previews

`src/core/src/previews/`：Live Preview 背后的 registry。它记录哪些本地端口/文件正被预览、使用哪些 slugs、生命周期如何。

## 职责

跟踪 preview sessions（dev-server ports 和 Markdown files），为 Server/Markdown 铸造 owner 与 Share 身份，执行共享访问期限，并清理 preview 相关进程。HTTP 侧（owner shell、Server routing、Share gate，以及无需子静态服务的 Markdown 直接渲染）在 [server](server.md) 的 `preview` 子模块里。

## 关键类型

| Type | File | Role |
|---|---|---|
| Preview store / `SESSIONS` | `store.rs` | Slug → preview session；`SHARE_TTL_SECS = 600` |
| Owner vs Share semantics | `mod.rs`、`store.rs` | 每种目标都有稳定的 owner slug；其 Share ID、访问码和授信组成一笔 600 秒事务 |
| `kill_by_session` / `shutdown_kill_all_ports` | `mod.rs` | 按 agent session / daemon stop 时所有 previewed ports 杀 dev-server processes |

## 交互

- **← server (MCP `preview`)：** agents 传入 dev-server port 或 Markdown file；`va-preview` skill 包装这个统一工具。
- **← server (`preview/` handlers)：** resolve slugs、render owner picker 与 Markdown content；本地 owner 直接加载 Server origin，隧道上的 Server 页面则使用受限代理。
- **← workspace：** 关闭 thread 会 kill 绑定到其 session 的 previews。
- **← cli / dashboard：** list 和 delete。

## 不变量：不要破坏

1. **每笔 Share 都是一笔限定作用域的事务**：一个 Preview、一个不透明 URL ID、一个可重复使用的六位访问码、一个浏览器授信和一个硬 TTL。Server Share 只接受 GET/HEAD iframe 导航与浏览器通过 `Sec-Fetch-Dest` 声明的静态子资源，必须拒绝非 GET/HEAD、fetch/XHR/EventSource、worker、WebSocket 和 HMR；它是页面预览传输，不是通用 API 兼容层或 API 隔离沙盒。`/va/*`、owner、chat 与 review 不进入 Share。不重新审视[安全模型](../../architecture/security-model.md)就不要扩大 target scope 或 lifetime。
2. **Preview processes 是 session-scoped**：agent session 的 dev servers 会随 `/close` 和 daemon 一起死，不留下 orphaned `npm run dev`。
3. 远程 Server 与 Markdown owner link 需要 owner 配对；Share expiry 不能影响 owner path。

## 已知技术债

- remediation plan 中无跟踪项。

---

*Source anchors: `src/core/src/previews/` (mod, store), `src/server/src/web_server/preview/` (iframe, markdown, access), `src/server/src/web_server/mcp/tools.rs` (preview tools).*
*Last verified: v0.7.24*

<sub>[◀ Module: pty](pty.md) · [文档索引](../../README.md) · [Module: tunnels ▶](tunnels.md)</sub>
