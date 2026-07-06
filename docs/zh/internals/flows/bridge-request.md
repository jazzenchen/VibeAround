# Flow: Bridge 请求

跟踪 launched agent CLI 发出的一次模型 API 调用，看它如何通过本地 bridge 到达 provider，再返回。概念说明见[本地 API 与 Bridge](../../architecture/local-api-and-bridge.md)；本页只讲请求路径本身。

## 逐跳

```text
agent CLI ──1─► /va/local-api/{profile}/{scope}/{api_type}/v1/…
                    │2 decode (client dialect → universal)
                    │3 model mapping (alias → upstream id)
                    │4 content policy (capability sanitize)
                    │5 web-search handling
                    ▼
                encode + send ──6──► provider endpoint
                    ▼
                translate back ──7──► streaming response to the CLI
```

**1. CLI 调 localhost。** 通过 bridged profile 启动时，会把类似 `http://127.0.0.1:12358/va/local-api/moonshot/codex-openai-responses/openai-responses/v1` 的 base URL 渲染进 agent config。CLI 以为这就是它的 vendor API。local-bridge gate 会拒绝非 loopback 调用；body 最大 64 MB。
→ `src/core/src/profiles/bridge_launch.rs` (URL rendering), `src/server/src/web_server/auth.rs` (`require_local_bridge`)

**2. Decode。** `{api_type}` 表示客户端方言；匹配的 translator（OpenAI Responses / OpenAI Chat / Anthropic Messages / Gemini）把请求解码成 universal request。若 client 和 provider 方言相同且不需要重写，bridge 可以走 passthrough path。
→ `src/server/src/web_server/api_bridge/protocol.rs`, `passthrough.rs`

**3. Model mapping。** 请求的 model id 会过 profile 的 model routes：alias ids（为那些会校验 vendor model list 的 CLI 铸造）映射到真实 upstream model；route preference 也决定使用 profile 的哪个 endpoint/dialect。
→ `api_bridge/model_mapping.rs`

**4. Content policy。** 请求会按 target model 在 catalog 里声明的 capability 做 sanitize，例如 text-only model 会剥掉 image parts。这样 mismatch 会降级，而不是上游直接拒绝。
→ `api_bridge/content_policy.rs`, `src/resources/profile-catalog/` (capabilities)

**5. Web search handling。** 如果请求带 provider-native web-search tool，且配置了 host-side search（或 `replace_provider_web_search` 强制替换），bridge 会换成 VibeAround 的 search tool，让结果来自你配置的 sources。
→ `api_bridge/server_tools.rs`, `src/core/src/search.rs`

**6. Upstream。** Universal request 编码成 provider 方言，附上 profile 里的真实 API key（只存在 daemon 侧，绝不写进 CLI config），并在 rate-limit response 上做有界 retry。Provider quirks（DeepSeek 等）由 adapters 处理；Gemini 的 OAuth code-assist 路径有自己的 adapter。
→ `api_bridge/upstream.rs`, `google_code_assist.rs`, the `va-ai-api-bridge` crate (translators)

**7. Stream back。** Provider stream events 会逐 chunk 翻译回客户端方言的 streaming format（非 streaming call 则 buffer）。如果 launch popup 的 recorder 开启，请求/响应 body 会被内存捕获用于调试，不持久化。
→ `api_bridge/stream.rs`, `completion.rs`, `src/server/src/web_server/bridge_recording.rs`

## Agent-as-API 变体

`/va/local-agent/{agent}/{profile}/v1/…` 走同样的 decode 步骤，但 universal request 不发往上游 provider，而是变成一个**给 hosted agent process 的 prompt**，包含 tools、workspace 等；agent 输出再编码回 chat/responses 风格 reply。同一个 gate，同一组方言，不同 executor。
→ `api_bridge/local_agent.rs`

## 失败行为

| 失败 | 结果 |
|---|---|
| Daemon 没运行 | CLI 看到 connection refused；bridged profiles 需要 daemon 活着 |
| 未知 profile / api_type | 客户端方言里的 4xx JSON error |
| 上游 429 | 有界 backoff retry，随后错误被翻译回客户端 |
| 上游 auth 失败 | 翻译成 401；应检查 profile 的 key，而不是 CLI |
| Model 缺少请求的 capability | 请求被 sanitize（降级），不是上游 rejection |

---

*Source anchors: `src/server/src/web_server/api_bridge/` (protocol, routes, model_mapping, content_policy, server_tools, upstream, stream, completion, local_agent), `src/core/src/profiles/bridge_launch.rs`, `src/server/src/web_server/bridge_recording.rs`.*
*Last verified: v0.7.11*

<sub>[◀ Flow: 权限请求](permission.md) · [文档索引](../../README.md) · [Flow: Agent 启动 ▶](native-launch.md)</sub>
