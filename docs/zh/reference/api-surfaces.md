# API 面参考

守护进程的可编程面：给 Agent 的 MCP 工具、给模型客户端的本地 API 路由，以及 WebSocket 端点。HTTP `/api/*` REST 路由是控制台和 `va-client` 消费的内部契约，尚不是稳定的公开 API。

## MCP 工具

在 `/mcp` 提供（streamable HTTP 上的 JSON-RPC，token 认证）。`integrations.mcp_auto_install` 开启时自动注入已启用 Agent 的全局配置。

| 工具 | 用途 |
|---|---|
| `get_session_id` | 解析调用方 Agent 会话的身份 |
| `prepare_handover` | 签发接续码（4 字符、120 秒 TTL、一次性），用于跨界面连续性 |
| `register_workspace` | 把当前项目目录注册为 Workspace |
| `initialize_subagents` | 开始多 Agent 回合 —— 模式：`parallel`、`collaboration`、`brainstorming` |
| `wait_for_subagents` | 阻塞到子 Agent 报告完成；返回它们的报告 |
| `preview` | 为某个 dev server 端口创建实时预览 |
| `md_preview` | 创建 Markdown 渲染预览 |

按 Agent 安装的配套技能（`skill_auto_install`）：`vibearound`（交接）、`va-session`、`va-preview`、`va-md-preview`、`agent-collaboration`。

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

| URL | 认证 | 寿命 |
|---|---|---|
| `/preview/u/{slug}` | Owner token | 预览存在期间 |
| `/preview/s/{slug}` | 无 | 600 秒 |
| `/md-preview/{slug}` | Owner token | 存在期间 |

---

*Source anchors: `src/server/src/web_server/mcp/mod.rs` (tool dispatch), `src/core/src/workspace/handover.rs` (code TTL), `src/server/src/web_server/api_bridge/routes.rs` + `mod.rs` (route table, body limit), `src/server/src/web_server/ws_domains.rs` (state endpoints), `src/core/src/previews/store.rs` (share TTL).*
*Last verified: v0.7.11*

<sub>[◀ CLI 参考](cli.md) · [文档索引](../README.md) · [计时器与上限 ▶](timers-and-limits.md)</sub>
