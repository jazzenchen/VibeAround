# Module: previews

`src/core/src/previews/`：Live Preview 背后的 registry。它记录哪些本地端口/文件正被预览、使用哪些 slugs、生命周期如何。

## 职责

跟踪 preview sessions（dev-server ports 和 rendered files），为 Server 铸造本地 owner URL、为 Markdown 铸造 owner/share 事务，执行共享访问期限，并清理 preview 相关进程。HTTP serving side（page proxy、iframe toolbar、Markdown rendering）在 [server](server.md) 的 `preview` 子模块里。

## 关键类型

| Type | File | Role |
|---|---|---|
| Preview store / `SESSIONS` | `store.rs` | Slug → preview session；`SHARE_TTL_SECS = 600` |
| Owner vs share semantics | `mod.rs`、`store.rs` | Server 只有本地 owner；Markdown owner 随 preview 存活，share ID、访问码和授信组成一笔 600 秒事务 |
| `kill_by_session` / `shutdown_kill_all_ports` | `mod.rs` | 按 agent session / daemon stop 时所有 previewed ports 杀 dev-server processes |

## 交互

- **← server (MCP `preview` / `md_preview`)：** agents 通过 tools 创建 previews；skills（`va-preview`、`va-md-preview`）包装它们。
- **← server (`preview/` handlers)：** resolve slugs、proxy requests、render markdown。
- **← workspace：** 关闭 thread 会 kill 绑定到其 session 的 previews。
- **← cli / dashboard：** list 和 delete。

## 不变量：不要破坏

1. **只有 Markdown 可以铸造分享事务**：单个文档、一个不透明 URL ID、一个可重复使用的六位访问码、一个浏览器授信和一个硬 TTL。Server preview 保持 loopback-only。不重新审视[安全模型](../../architecture/security-model.md)就不要扩大 target scope 或 lifetime。
2. **Preview processes 是 session-scoped**：agent session 的 dev servers 会随 `/close` 和 daemon 一起死，不留下 orphaned `npm run dev`。
3. 远程 Markdown owner link 需要 owner 配对；share expiry 不能影响 owner path。

## 已知技术债

- remediation plan 中无跟踪项。

---

*Source anchors: `src/core/src/previews/` (mod, store), `src/server/src/web_server/preview/` (proxy, iframe, markdown, cookie_proxy), `src/server/src/web_server/mcp/tools.rs` (preview tools).*
*Last verified: v0.7.11*

<sub>[◀ Module: pty](pty.md) · [文档索引](../../README.md) · [Module: tunnels ▶](tunnels.md)</sub>
