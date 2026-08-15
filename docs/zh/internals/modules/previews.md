# Module: previews

`src/core/src/previews/`：Live Preview 背后的 registry。它记录哪些本地端口/文件正被预览、使用哪些 slugs、生命周期如何。

## 职责

在内存中跟踪当前 daemon 的 preview sessions（dev-server ports 和 Markdown files），为 Server/Markdown 铸造 owner 与 Share 身份，执行共享访问期限，并清理已登记的 Server ports。HTTP 侧（owner shell、Server routing、Share gate，以及无需子静态服务的 Markdown 直接渲染）在 [server](server.md) 的 `preview` 子模块里。

## 关键类型

| Type | File | Role |
|---|---|---|
| Preview store / `SESSIONS` | `store.rs` | Slug → preview session；`SHARE_TTL_SECS = 600` |
| Owner vs Share semantics | `mod.rs`、`store.rs` | 每种目标都有稳定的 owner slug；其 Share ID、访问码和授信组成一笔 600 秒事务 |
| `delete_session` / `cleanup_registered_previews` | `mod.rs`、`registrations.rs` | 关闭单个 Preview / 在 daemon 启动与关闭时执行同一套 cleanup |

## 交互

- **← server (MCP `preview`)：** agents 传入 dev-server port 或 Markdown file；`va-preview` skill 包装这个统一工具。
- **← server (`preview/` handlers)：** resolve slugs、render owner picker 与 Markdown content；本地 Server owner 直接加载 loopback origin，远程 owner 使用透明 loopback 代理，Server Share 使用更窄的页面预览代理。
- **← cli / dashboard：** list 和 delete。

## 不变量：不要破坏

1. **Server owner 的行为刻意保持简单**：创建 Server iframe 前，owner SPA 会让用户针对每个 Preview、每个浏览器会话确认一次风险。本地 owner 直接加载 loopback origin；远程 owner 只把常规 HTTP 与 WebSocket/HMR 流量透明转发到 `127.0.0.1:<已登记端口>`，`/va/*` 保留给 VibeAround。不要给这条路径增加存活性、内容、workspace、process、header 或 redirect 审查。
2. **每笔 Share 都是一笔限定作用域的事务**：一个 Preview、一个不透明 URL ID、一个可重复使用的六位访问码、一个浏览器授信和一个硬 TTL。Server Share 会原样转发已认证的 GET/HEAD 路径，包括页面的数据读取；写请求、协议升级、service worker、WebSocket 与 HMR 必须保持不支持，`/va/*`、owner 页面、chat 与 review 不进入 Share。它是页面预览传输，不是通用 API 兼容层或 API 隔离沙盒；不要根据路径名称推断策略。不重新审视[安全模型](../../architecture/security-model.md)就不要扩大 target scope 或 lifetime。
3. **Preview 状态一律不恢复**：File 与 Server Preview 状态只存在于内存。最小 cleanup journal 只记录 File 标记与 Server port，用于退出中断后，让下一次启动重复关闭时的同一套 cleanup。Cleanup 会杀掉已登记的 Server port、删除 journal，绝不重建 Preview；关闭 thread/session 不处理 Preview。
4. **Agent 侧只有一个明确的刷新触发点**：agent 回合结束不会刷新 iframe。MCP `preview` 调用成功后，无提示刷新当前已打开的对应 owner Preview；用户手动刷新 Preview 时，只有存在将被清空的 review draft 才会确认。
5. 远程 Server 与 Markdown owner link 需要 owner 配对；Share expiry 不能影响 owner path。

## 已知技术债

- remediation plan 中无跟踪项。

---

*Source anchors: `src/core/src/previews/` (mod, store), `src/server/src/web_server/preview/` (iframe, markdown, access), `src/server/src/web_server/mcp/tools.rs` (preview tools).*
*Last verified: v0.7.24*

<sub>[◀ Module: pty](pty.md) · [文档索引](../../README.md) · [Module: tunnels ▶](tunnels.md)</sub>
