# API 面参考

守护进程的可编程面：给 Agent 的 MCP 工具、给模型客户端的本地 API 路由，以及 WebSocket 端点。HTTP `/api/*` REST 路由是控制台和 `va-client` 消费的内部契约，尚不是稳定的公开 API。

## MCP 工具

在 `/mcp` 提供（streamable HTTP 上的 JSON-RPC）。每次 daemon 启动都会在 `~/.vibearound/auth-mcp.json` 生成一个 MCP-only credential；每次通过 VibeAround 启动 Agent 时，都会把当前 credential 写入该 Agent 的项目级 MCP 配置。Dashboard/control API 不接受这个 credential。

| 工具 | 用途 |
|---|---|
| `va_mcp_get_session_id` | 解析调用方 Agent 会话的身份 |
| `va_mcp_prepare_handover` | 签发接续码（4 字符、120 秒 TTL、一次性），用于跨界面连续性 |
| `va_mcp_register_workspace` | 把当前项目目录注册为 Workspace |
| `va_mcp_initialize_subagents` | 开始多 Agent 回合 —— 模式：`parallel`、`collaboration`、`brainstorming` |
| `va_mcp_wait_for_subagents` | 阻塞到子 Agent 报告完成；返回它们的报告 |
| `va_mcp_preview` | 预览一个明确来源：正在运行的 dev-server `port` 或 Markdown `file`；Markdown 由 VibeAround 直接渲染，不会另起服务 |

每次启动还会用 bundled 版本替换 VibeAround 保留的项目级技能：`vibearound`（交接）、`va-session`、`va-preview`，以及受支持 Agent 的 `agent-collaboration`。

## 本地 API 路由族

仅回环地址，并由本地 bridge 检查把守；主路径 `/local-api` 和 `/local-agent` 还分别要求自己的凭证。请求体最大 64 MB。机制见[本地 API 与 Bridge](../architecture/local-api-and-bridge.md)。

```text
/va/local-api/{profile}/{scope}/{api_type}/v1/{responses | chat/completions | messages | models}
/va/local-agent/{agent}/{profile}/v1/{responses | chat/completions | messages | models}
/va/bridge/{profile}/{api_type}/v1/…            （旧版形状）
```

`{api_type}` ∈ `openai-responses` | `openai-chat` | `anthropic` | `gemini`。Gemini 客户端还额外获得 generateContent 形状的路由。

### 可直接复制的示例

把 `LOCAL_API_KEY` 设为 `~/.vibearound/local-api-auth.json` 中的 `token`，把 `LOCAL_AGENT_API_KEY` 设为 `~/.vibearound/local-agent-api-auth.json` 中的 `token`。两者都会在守护进程重启时轮换。桌面端的本地 API 面板也可直接复制 Agent-as-API key。

列出某个 Profile 提供的模型：

```bash
curl http://127.0.0.1:12358/va/local-api/moonshot/curl-test/openai-chat/v1/models \
  -H "Authorization: Bearer $LOCAL_API_KEY"
```

经 Bridge 的 chat completion（客户端说 OpenAI Chat；守护进程翻译成该 Profile 供应商说的方言）：

```bash
curl http://127.0.0.1:12358/va/local-api/moonshot/curl-test/openai-chat/v1/chat/completions \
  -H "Authorization: Bearer $LOCAL_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"model": "kimi-k2.7-code", "messages": [{"role": "user", "content": "hello"}]}'
```

Agent-as-API —— 同样的请求形状，但由托管的编程 Agent（带工具和 Workspace）而不是裸模型执行：

```bash
curl http://127.0.0.1:12358/va/local-agent/claude/direct/v1/chat/completions \
  -H "Authorization: Bearer $LOCAL_AGENT_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"model": "claude", "messages": [{"role": "user", "content": "what does this repo do?"}]}'
```

任一请求体加 `"stream": true` 即可 SSE 流式。`{scope}` 路径段（上例的 `curl-test`）是自由格式的启动元数据 —— 手动调用时任何 URL 安全的字符串都行。

## WebSocket 端点

全部 token 认证；负载细节见[架构总览](../architecture/overview.md#通信路径)。

| 端点 | 用途 |
|---|---|
| `/ws?session_id=` | 终端字节 + JSON resize（Web 终端 ↔ PTY） |
| `/ws/chat` | Web/TUI 聊天事件 |
| `/ws/channels`、`/ws/tunnels`、`/ws/sessions`、`/ws/agents/runtime` | 实时状态：每次变化发全量快照 |

## 预览 URL

| URL | 目标 | 认证 | 寿命 |
|---|---|---|---|
| `/preview/u/{slug}` | Server 或 Markdown 的 Owner shell | 回环地址或已配对 owner | 预览存在期间 |
| `/preview/u/{slug}/content` | 选中的 owner 内容；本地 Server 直接使用其 loopback origin | 与 owner shell 相同 | 预览存在期间 |
| `/preview/s/{share_id}` | Server 或 Markdown Share | 六位访问码，随后使用限定作用域的浏览器授信 | 共用 600 秒期限 |

Server Share 代理会在每次请求时重新验证限定作用域的浏览器授信，并原样转发已认证的 GET/HEAD 路径，包括页面的数据读取。写请求、协议升级、service worker、WebSocket 与 HMR 暂不支持；`/va/*`、owner 页面、chat 和审阅控件不进入 Share。它是页面预览传输，不是通用 API 兼容层或 API 隔离沙盒；已接受的 GET/HEAD 路径不会按名称分类。

---

*Source anchors: `src/server/src/web_server/mcp/mod.rs` (tool dispatch), `src/core/src/workspace/handover.rs` (code TTL), `src/server/src/web_server/api_bridge/routes.rs` + `mod.rs` (route table, body limit), `src/server/src/web_server/ws_domains.rs` (state endpoints), `src/core/src/previews/store.rs` (share TTL).*
*Last verified: v0.7.24*

<sub>[◀ CLI 参考](cli.md) · [文档索引](../README.md) · [计时器与上限 ▶](timers-and-limits.md)</sub>
