# 供应商端点参考

内置 Profile 目录里的每个端点分组：套餐、区域、方言、base URL 和当前模型集合 —— 以及区分"长得像"端点的凭据语义。按供应商一行的概览在[支持矩阵](../product/supported-matrix.md)；Profile 的用法见[模型 Profile 指南](../guides/model-profiles.md)。

两条跨供应商通用的规则：

- **套餐是凭据，不只是 URL。** 多家供应商在相同或相似的 base URL 上同时提供按量付费 API 和订阅套餐（编程套餐、Token 套餐、Agent 套餐）—— 区别在于接受哪种 key。按你实际购买的凭据选对应的端点分组。
- **模型列表是精选的。** 目录只带每个端点当前的旗舰/编程模型，不是供应商的完整市场。缺的模型用相同 base URL 的 custom Profile 就能用。

## Kimi / Moonshot（`moonshot`）

| 端点分组 | 方言 | Base URL | 关键模型 |
|---|---|---|---|
| `moonshot-global` | openai-chat, anthropic | `https://api.moonshot.ai/v1` · `/anthropic` | `kimi-k2.7-code`、`kimi-k2.7-code-highspeed`、`kimi-k2.6`、`kimi-k2.5`、`moonshot-v1-*` |
| `moonshot-cn` | openai-chat, anthropic | `https://api.moonshot.cn/v1` · `/anthropic` | 同一家族 |
| `kimi-coding` | openai-chat, anthropic | `https://api.kimi.com/coding/v1` · `/coding/` | `kimi-for-coding` |

- Kimi Coding 是单独的产品，有自己的 key 和唯一在册模型 —— 不是 Moonshot API 的一个区域。
- K2.x 模型是 256K 上下文、多模态；K2.7 Code 始终使用思考。

## 阿里云百炼 DashScope（`dashscope`）

| 端点分组 | 方言 | Base URL | 关键模型 |
|---|---|---|---|
| `api-cn-beijing` | openai-chat, openai-responses, anthropic | `https://dashscope.aliyuncs.com/compatible-mode/v1` · `/apps/anthropic` | `qwen3.7-max/plus`、`qwen3.6-flash`、`deepseek-v4-pro`、`kimi-k2.6`、`glm-5.2`、`MiniMax-M2.5` |
| `coding-plan`（国际） | openai-chat, anthropic | `https://coding-intl.dashscope.aliyuncs.com/v1` · `/apps/anthropic` | `qwen3.7-plus`、`qwen3.6-plus`、`kimi-k2.5`、`glm-5`、`MiniMax-M2.5` |
| `coding-plan-cn` | openai-chat, anthropic | `https://coding.dashscope.aliyuncs.com/v1` · `/apps/anthropic` | 与国际版相同 |
| `token-plan-cn` | openai-chat, openai-responses, anthropic | `https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1` · `/apps/anthropic` | 宽泛的旗舰集合，含第三方模型 |

- 按量付费、编程套餐、Token 套餐是三种不同凭据；编程/Token 套餐的 key 在普通 API 端点上不可用，反之亦然。
- Responses 方言仅限具备工具能力的 Qwen 子集（`qwen3.7-max/plus`、`qwen3.6-plus/flash`）—— 内置搜索/代码/网页工具需要它。
- 百炼的国际区域端点（美国 / 新加坡 / 欧盟工作区域名）阿里有文档但不在目录里 —— 需要就用 custom Profile。

## DeepSeek（`deepseek`）

| 端点分组 | 方言 | Base URL | 关键模型 |
|---|---|---|---|
| 默认 | openai-chat, anthropic | `https://api.deepseek.com` · `/anthropic` | `deepseek-v4-pro`、`deepseek-v4-flash` |

- 注意 OpenAI base **没有 `/v1`** 后缀。
- Anthropic 兼容端点官方映射 Claude 档位：Opus 级请求 → `v4-pro`，Sonnet/Haiku 级 → `v4-flash`。那里不支持图片/文档内容块。

## MiniMax（`minimax`）

| 端点分组 | 方言 | Base URL | 关键模型 |
|---|---|---|---|
| `api-global` | openai-chat, openai-responses, anthropic | `https://api.minimax.io/v1` · `/anthropic` | `MiniMax-M3`、`M2.7(-highspeed)`、`M2.5(-highspeed)` |
| `api-cn` | openai-chat, anthropic | `https://api.minimaxi.com/v1` · `/anthropic` | 同一家族 |
| `token-plan-global` | openai-chat, openai-responses, anthropic | 与 api-global 相同 base | 同一家族 |
| `token-plan-cn` | openai-chat, anthropic | 与 api-cn 相同 base | 同一家族 |

- API 和 Token 套餐共用 base URL；分组存在的原因是按量付费 API key 和 Token 套餐订阅 key 是**不同凭据**。
- `MiniMax-M3` 是旗舰：1M 上下文、图像/视频输入、思考；Responses 方言增加推理控制。

## 小米 MiMo（`mimo`）

| 端点分组 | 方言 | Base URL | 关键模型 |
|---|---|---|---|
| `pay-as-you-go` | openai-chat, anthropic | `https://api.xiaomimimo.com/v1` · `/anthropic` | `mimo-v2.5-pro`、`mimo-v2.5`、`mimo-v2-pro`、`mimo-v2-omni`、`mimo-v2-flash` |
| `token-plan-cn` | openai-chat, anthropic | `https://token-plan-cn.xiaomimimo.com/v1` · `/anthropic` | 同一家族 |

- `mimo-v2.5-pro` 是推荐的编程默认（1M 上下文、128K 输出、联网搜索、函数调用）；`v2-omni` 增加全模态输入。
- Token 套餐的新加坡/阿姆斯特丹区域端点上游存在但不在目录 —— 需要就 custom Profile。

## 火山引擎 / 方舟（`volcengine`）

| 端点分组 | 方言 | Base URL | 关键模型 |
|---|---|---|---|
| `ark-api` | openai-chat, openai-responses, anthropic | `https://ark.cn-beijing.volces.com/api/v3` · `/api/compatible` | 带版本的部署 id：`doubao-seed-2-0-code-preview-260215`、`…-pro-260215`、`…-lite-260428`、`deepseek-v4-pro-260425` |
| `coding-plan` | openai-chat, anthropic | `…/api/coding/v3` · `/api/coding` | 套餐别名：`ark-code-latest`、`doubao-seed-2.0-code/pro`、`minimax-m3`、`glm-5.2`、`deepseek-v4-pro`、`kimi-k2.6` |
| `agent-plan` | openai-chat, anthropic | `…/api/plan/v3` · `/api/plan` | 同一别名集合 |

- 方舟 API 用**带版本的部署 id**；套餐用滚动别名（`ark-code-latest`）—— 两种 id 风格在端点分组之间不通用。
- 套餐在一份火山凭据下暴露多家旗舰第三方模型（MiniMax、GLM、DeepSeek、Kimi）。

## Z.AI / GLM（`zai`）

| 端点分组 | 方言 | Base URL | 关键模型 |
|---|---|---|---|
| `global` | openai-chat | `https://api.z.ai/api/paas/v4` | `glm-5.2`、`glm-5-turbo`、`glm-5v-turbo`、`glm-5.1`、`glm-4.7`、`glm-4.5-air` |
| `cn` | openai-chat | `https://open.bigmodel.cn/api/paas/v4` | 同一家族 |
| `coding-global` | openai-chat, anthropic | `…/api/coding/paas/v4` · `https://api.z.ai/api/anthropic` | `glm-5.2`、`glm-5-turbo`、`glm-4.7`、`glm-4.5-air` |
| `coding-cn` | openai-chat, anthropic | `…/api/coding/paas/v4` · `https://open.bigmodel.cn/api/anthropic` | 同一集合 |

- Anthropic 兼容端点属于**编程套餐产品**，不属于通用 API —— 通用 API key 在那里无法认证。

## Google Gemini / Vertex AI（`gemini`）

| 端点分组 | 方言 | Base URL | 关键模型 |
|---|---|---|---|
| `gemini-api` | gemini, openai-chat | `https://generativelanguage.googleapis.com`（+`/v1beta/openai`） | `gemini-3.1-pro-preview`、`gemini-3-flash-preview`、`gemini-2.5-pro/flash(-lite)`、`gemini-3.1-flash-lite` |
| `google-accounts` | gemini | `https://cloudcode-pa.googleapis.com` | 同一家族 |
| `vertex-openai-compatible` | openai-chat | 用户自填 | `google/…` 前缀 id |

- 三种认证形态：`gemini-api` = API key；`google-accounts` = Google OAuth（code-assist 流程，由专用适配器处理）；Vertex = 你自己的端点 URL 加 `google/` 前缀的模型 id。

## xAI / Grok（`xai`）

| 端点分组 | 方言 | Base URL | 关键模型 |
|---|---|---|---|
| 默认 | openai-chat, openai-responses | `https://api.x.ai/v1` | `grok-4.3`、`grok-build-0.1` |

## NVIDIA NIM（`nvidia`）

| 端点分组 | 方言 | Base URL | 关键模型 |
|---|---|---|---|
| 默认 | openai-chat | `https://integrate.api.nvidia.com/v1` | `nvidia/nemotron-3-super-120b-a12b`、`nvidia/nemotron-3-nano-30b-a3b`、`qwen/qwen3-coder-480b`、`openai/gpt-oss-120b`、`moonshotai/kimi-k2.6` |

## OpenRouter（`openrouter`）

| 端点分组 | 方言 | Base URL | 关键模型 |
|---|---|---|---|
| 默认 | openai-chat | `https://openrouter.ai/api/v1` | `anthropic/claude-sonnet-4.5`、`openai/gpt-4o`、`deepseek/deepseek-chat`、`google/gemini-2.5-pro` |

- 目录只带四个代表性 id；OpenRouter 的整个市场都可用 —— 路由器接受的任何模型 id 都能填。

## Azure OpenAI（`azure`）

| 端点分组 | 方言 | Base URL | 关键模型 |
|---|---|---|---|
| 默认 | openai-responses | 用户自填资源 URL | 用户自填部署名 |

- Azure 端点是按资源的（`https://{resource}.openai.azure.com/openai/v1/`），模型是你的部署名 —— 目录有意让两个字段留空。

---

**来源。** Base URL 和模型集合镜像 `src/resources/profile-catalog/`（随包发行的目录）；套餐/凭据语义在 2026-06-18 的审计中对照官方供应商文档验证（`reports/provider-plan-api-audit-2026-06-18.md`、`reports/provider-profile-official-verification-2026-06-18.md`）。目录刻意不带模型发现元数据 —— 模型列表是精选的，超出部分用 custom Profile 覆盖。

*Source anchors: `src/resources/profile-catalog/*.json` (endpoints, base URLs, models), `src/core/src/profiles/catalog.rs` (loader), `reports/provider-plan-api-audit-2026-06-18.md` + `reports/provider-profile-official-verification-2026-06-18.md` (official-doc verification).*
*Last verified: v0.7.11*

<sub>[◀ 计时器与上限](timers-and-limits.md) · [文档索引](../README.md) · [Internals（英文） ▶](../../internals/README.md)</sub>
