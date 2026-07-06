# 模型 Profile 指南

模型 Profile 保存一份供应商凭据加路由规则，让 Agent 无需了解供应商的任何细节就能使用它。本页讲如何创建和使用 Profile；背后的机制见[本地 API 与 Bridge](../architecture/local-api-and-bridge.md)。

## 什么时候需要 Profile，什么时候不需要

- **不需要 Profile**（`direct`）：每个 Agent 用自己的官方登录（Claude Code 用你的 Anthropic 账号，Codex 用 OpenAI）。VibeAround 托管 Agent，但不介入模型链路。
- **需要 Profile**：你想用第三方或自选的供应商 —— 给 Codex 配 Kimi、给 Claude Code 配 DeepSeek、一份订阅供所有 Agent 使用，或一个自定义的 OpenAI 兼容端点。

## 创建 Profile

在桌面应用的模型 Profile 页面（或通过控制台）：

1. **选供应商。** 目录内置 Kimi/Moonshot、DashScope、DeepSeek、OpenRouter、MiniMax、MiMo、火山引擎、Z.AI/GLM、Gemini/Vertex、xAI、NVIDIA NIM、Azure OpenAI —— 端点、模型和上下文窗口都已预填。也可以选 **custom** 接任何兼容端点。
2. **选端点变体**（同一供应商往往有多个：全球 vs 国内、按量付费 vs 编程套餐）。变体之间 base URL、可用模型、有时连方言都不同 —— 按供应商的完整拆解见[供应商端点参考](../reference/provider-endpoints.md)。
3. **粘贴 API key。** Key 只存在本地，只会由守护进程发给对应供应商。**Key 要和套餐匹配**：一些供应商给按量付费和编程/Token 套餐发不同凭据，哪怕 base URL 看起来一样。
4. **选模型。** 决定这个 Profile 暴露哪些上游模型、默认用哪个。对校验模型名的 Agent，Profile 可以定义别名模型 id，映射到真实的上游模型。

Profile 在运行时管理 —— 创建、编辑、排序、删除 —— 无需重启守护进程。CLI 用 `va profiles` 列出。

## 使用 Profile

**托管对话（IM / Web Chat）。** 按渠道设默认值（`remote.channels.<kind>.profile_id`）、按 Thread 切换（`/switch host <agent> <profile>` 或 `/profile --switch <id>`），或在 Web Chat 启动时选择。托管的 Agent 进程会拿到指向本地 Bridge 的环境和配置。

**终端启动。** 在桌面 Launch 页面选 Agent + Profile，或用保存的启动配置（`va launch --profile <name>`）。渲染出的配置让 Agent 与 `http://127.0.0.1:12358/va/local-api/...` 通信；守护进程负责翻译到供应商。见 [Agent 启动指南](agent-launch.md)。

**任何 OpenAI 兼容工具。** 把工具指向 Profile 的本地端点（Profile UI 里有显示），就能获得同样的翻译 —— 非 Agent 工具也因此可以共享你的供应商配置。

## 选择 API 类型搭配

每个 Profile 端点声明自己的上游方言，每个 Agent 说自己的客户端方言。Bridge 能翻译任意组合，但有两条经验法则：

- **能对齐方言就对齐**（比如给 Claude Code 用 Kimi 的 `anthropic` 端点）—— 直通比翻译更忠实，尤其涉及供应商特有功能时。
- **其余交给目录决定。** 目录条目标注了每个端点最适合哪种方言；启动流程会选一条合理的路由（`bridge_route_preference`），除非你手动覆盖。

模型能力也重要：Bridge 会按目标模型声明的能力清洗请求（比如给纯文本模型丢弃图片部分），所以能力不匹配是优雅降级而不是报错 —— 但选一个具备你的 Agent 所需能力（图像输入、长上下文）的模型能避免意外。

## 网页搜索替代

没有原生联网搜索的模型也能搜索：启用宿主侧搜索工具（设置里的 `search_tool`），并可选开启 `api_bridge.replace_provider_web_search`，让具备原生搜索的模型也走你配置的搜索源。一份搜索配置服务所有 Agent 和 Profile。

## Profile 故障排查

| 症状 | 可能原因 |
|---|---|
| Agent 说找不到模型 | 该 Agent 校验模型 id —— 用 Profile 的别名模型 id，不要用上游 id |
| 供应商返回 401 | Key 无效/过期，或端点变体不对（全球 key 用在了国内端点） |
| 回复正常但图片被忽略 | 目标模型不支持图像输入；查 Profile 里该模型的能力 |
| 高负载下频繁限流 | Bridge 会自动带退避重试；持续 429 说明套餐配额就是上限 |
| Agent 启动了但报 `ConnectionRefused` | 守护进程没在运行 —— Bridge 化的 Profile 需要它活着；启动桌面应用或 `va serve` |

---

*Source anchors: `src/resources/profile-catalog/` (providers, endpoints, models, capabilities), `src/core/src/profiles/` (schema, catalog, render, bridge_launch), `src/server/src/web_server/api_bridge/` (model_mapping, content_policy, rate-limit retry), `src/core/src/config.rs` (search_tool, replace_provider_web_search).*
*Last verified: v0.7.11*

<sub>[◀ 连接渠道](connect-channels.md) · [文档索引](../README.md) · [Agent 启动指南 ▶](agent-launch.md)</sub>
