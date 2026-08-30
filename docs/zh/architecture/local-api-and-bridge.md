# 本地 API 与 Bridge

VibeAround 自带模型 API Bridge：一个本地转换层，让任何受支持的客户端 API 方言与任何已配置的供应商对话。"一份 Kimi 订阅驱动 Codex、Claude Code 和 Gemini CLI"就是靠它实现的。本页解释 Bridge 是什么、请求如何流经它；Profile 的配置方法见[模型 Profile 指南](../guides/model-profiles.md)。

## Bridge 解决什么问题

Agent CLI 与厂商方言硬绑定：Codex 说 OpenAI Responses API，Claude Code 说 Anthropic Messages，Gemini CLI 说 Gemini GenerateContent。而模型供应商各自只暴露自己的一种方言。没有转换的话，选了供应商就等于选定了 Agent。

Bridge 把两者解耦。它在本地端点上接受**客户端**方言的请求，转换为统一的内部表示，再按**供应商**方言编码发往上游 —— 包括双向的流式传输。

## 支持的协议

四种方言两侧都能转换：

| API type id | 方言 |
|---|---|
| `openai-responses` | OpenAI Responses API |
| `openai-chat` | OpenAI Chat Completions |
| `anthropic` | Anthropic Messages |
| `gemini` | Gemini GenerateContent |

任意客户端方言可以搭配任意供应商方言。客户端和供应商方言一致时，Bridge 可以近乎不动地直通请求。

## 端点族

所有 Bridge 端点由本地守护进程在 `/va/` 下提供，只接受本地的、已认证的调用方（路由表见 [API 面参考](../reference/api-surfaces.md#local-api-route-families)）：

```text
/va/local-api/{profile}/{scope}/{target_api_type}/v1/…    # Profile 作用域 bridge（主路径）
/va/local-agent/{agent_id}/{profile_id}/v1/…               # agent-as-API
/va/bridge/{profile_id}/{target_api_type}/v1/…             # 旧版形状
```

每族都暴露客户端期望的标准子路径：`/v1/chat/completions`、`/v1/responses`、`/v1/messages`、`/v1/models`，以及 Gemini 的 generateContent 路由。

**Profile 作用域 bridge（`local-api`）。** 主路径。`{profile}` 选定凭据和供应商，`{scope}` 标识这个 URL 是为哪次启动/哪个客户端签发的，`{target_api_type}` 声明客户端的方言。通过 Bridge 化的 Profile 启动 Agent 时，VibeAround 把这些 URL 直接渲染进 Agent 的配置 —— Codex 拿到 `/openai-responses/v1` 下的 `base_url`，Claude Code 拿到 Anthropic 形状的端点，谁都不知道真正的供应商是谁。

**Agent-as-API（`local-agent`）。** 把一个托管的编程 Agent 本身变成 OpenAI/Anthropic 兼容端点：请求变成发给真实 Agent（带工具和 Workspace）的提示，响应以所请求的方言流回。任何 OpenAI 兼容工具都能借此驱动一个完整的编程 Agent。

主路径 Profile Bridge 与 Agent-as-API 使用彼此独立、随守护进程轮换的凭证。拿到 `~/.vibearound/auth.json` 中 `bridge_token` 的客户端只能调用 `/local-api`，不能启动 Agent；Agent-as-API 客户端使用同一文件里的 `agent_token`。

## 一个请求经历了什么

1. **解码**：从客户端方言解码为统一请求。
2. **模型映射。** 请求的模型 id 经 Profile 的模型路由映射 —— 包括 Agent 非要不可的"假"模型 id（只接受 `gpt-*` 名字的 Agent 可以拿到一个映射到上游 `kimi-k2.7-code` 的别名）。
3. **内容策略。** 请求内容按上游模型声明的能力清洗 —— 比如目标模型不支持图像输入时，图片部分被剥除或拒绝。
4. **网页搜索处理。** 供应商原生的搜索工具可以被 VibeAround 宿主侧搜索替换（`replace_provider_web_search`），没有原生搜索的模型也能拿到真实结果，且所有 Agent 共享一份搜索配置。
5. **编码并发送**到上游，带 Profile 的凭据；限流响应有限次重试。
6. **把响应翻译**（或流式翻译）回客户端方言。实时记录器可以为启动弹窗的调试视图捕获请求/响应体 —— 只在内存中，从不持久化。

## 供应商说明

供应商细节在 Profile 目录里（Moonshot/Kimi、DashScope/Qwen、DeepSeek、OpenRouter、MiniMax、MiMo、火山引擎、Z.AI/GLM、Gemini、xAI、NVIDIA、Azure OpenAI）。目录条目声明端点、方言、模型、上下文窗口和能力，所以支持供应商新模型是一次目录更新，不是代码改动。DeepSeek 等若干供应商有专用适配器处理转换的特殊之处。

对上游供应商的认证是按 Profile 的静态 API key。OAuth/订阅式上游认证（例如 Gemini 的 code-assist 流程）在受支持的场合由专用适配器处理。

## 信任边界

Bridge 绑定回环接口并要求本地 bridge 门禁；主路径 `/local-api` 和 `/local-agent` 还分别要求自己的 scoped credential。隧道无法触达它。请求体最大接受 64 MB，以容纳大上下文负载。供应商凭据不会出现在渲染给客户端的配置里；真正的 key 由守护进程注入上游请求。

---

*Source anchors: `src/server/src/web_server/api_bridge/` (protocol, routes, model_mapping, content_policy, upstream, local_agent), `src/core/src/profiles/bridge_launch.rs` (URL rendering), `src/resources/profile-catalog/` (providers), `src/server/src/web_server/mod.rs` (route table, body limit).*
*Last verified: v0.7.11*

<sub>[◀ 渠道插件系统](channel-plugin-system.md) · [文档索引](../README.md) · [安全模型 ▶](security-model.md)</sub>
