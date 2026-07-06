# Provider endpoints reference

Every endpoint group in the built-in profile catalog: plans, regions, dialects, base URLs, and current model sets — plus the credential semantics that distinguish look-alike endpoints. The one-line-per-provider overview is in the [supported matrix](../product/supported-matrix.md); how to use profiles is the [model profiles guide](../guides/model-profiles.md).

Two rules that apply across providers:

- **A plan is a credential, not just a URL.** Several providers serve pay-as-you-go API and subscription plans (Coding Plan, Token Plan, Agent Plan) on the same or similar base URLs — the difference is which kind of key is accepted. Pick the endpoint group matching the credential you actually bought.
- **Model lists are curated.** The catalog carries current flagship/coding models per endpoint, not the provider's full marketplace. Anything missing works via a custom profile with the same base URL.

## Kimi / Moonshot (`moonshot`)

| Endpoint group | Dialects | Base URL | Key models |
|---|---|---|---|
| `moonshot-global` | openai-chat, anthropic | `https://api.moonshot.ai/v1` · `/anthropic` | `kimi-k2.7-code`, `kimi-k2.7-code-highspeed`, `kimi-k2.6`, `kimi-k2.5`, `moonshot-v1-*` |
| `moonshot-cn` | openai-chat, anthropic | `https://api.moonshot.cn/v1` · `/anthropic` | same family |
| `kimi-coding` | openai-chat, anthropic | `https://api.kimi.com/coding/v1` · `/coding/` | `kimi-for-coding` |

- Kimi Coding is a separate product with its own key and a single documented model — not a region of the Moonshot API.
- K2.x models are 256K-context and multimodal; K2.7 Code always uses thinking.

## Alibaba DashScope / Bailian (`dashscope`)

| Endpoint group | Dialects | Base URL | Key models |
|---|---|---|---|
| `api-cn-beijing` | openai-chat, openai-responses, anthropic | `https://dashscope.aliyuncs.com/compatible-mode/v1` · `/apps/anthropic` | `qwen3.7-max/plus`, `qwen3.6-flash`, `deepseek-v4-pro`, `kimi-k2.6`, `glm-5.2`, `MiniMax-M2.5` |
| `coding-plan` (intl) | openai-chat, anthropic | `https://coding-intl.dashscope.aliyuncs.com/v1` · `/apps/anthropic` | `qwen3.7-plus`, `qwen3.6-plus`, `kimi-k2.5`, `glm-5`, `MiniMax-M2.5` |
| `coding-plan-cn` | openai-chat, anthropic | `https://coding.dashscope.aliyuncs.com/v1` · `/apps/anthropic` | same as intl |
| `token-plan-cn` | openai-chat, openai-responses, anthropic | `https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1` · `/apps/anthropic` | broad flagship set incl. third-party models |

- Pay-as-you-go, Coding Plan, and Token Plan are three different credentials; Coding/Token Plan keys do not work on the plain API endpoint and vice versa.
- The Responses dialect is limited to the tool-capable Qwen subset (`qwen3.7-max/plus`, `qwen3.6-plus/flash`) — built-in search/code/web tools require it.
- Bailian's international regional endpoints (US / Singapore / EU workspace domains) are documented by Alibaba but not in the catalog — use a custom profile if you need them.

## DeepSeek (`deepseek`)

| Endpoint group | Dialects | Base URL | Key models |
|---|---|---|---|
| default | openai-chat, anthropic | `https://api.deepseek.com` · `/anthropic` | `deepseek-v4-pro`, `deepseek-v4-flash` |

- Note the OpenAI base has **no `/v1`** suffix.
- The Anthropic-compatible endpoint maps Claude tiers officially: Opus-class requests → `v4-pro`, Sonnet/Haiku-class → `v4-flash`. Image/document content blocks are not supported there.

## MiniMax (`minimax`)

| Endpoint group | Dialects | Base URL | Key models |
|---|---|---|---|
| `api-global` | openai-chat, openai-responses, anthropic | `https://api.minimax.io/v1` · `/anthropic` | `MiniMax-M3`, `M2.7(-highspeed)`, `M2.5(-highspeed)` |
| `api-cn` | openai-chat, anthropic | `https://api.minimaxi.com/v1` · `/anthropic` | same family |
| `token-plan-global` | openai-chat, openai-responses, anthropic | same base as api-global | same family |
| `token-plan-cn` | openai-chat, anthropic | same base as api-cn | same family |

- API and Token Plan share base URLs; the split exists because pay-as-you-go API keys and Token Plan Subscription Keys are **different credentials**.
- `MiniMax-M3` is the flagship: 1M context, image/video input, thinking; the Responses dialect adds reasoning controls.

## Xiaomi MiMo (`mimo`)

| Endpoint group | Dialects | Base URL | Key models |
|---|---|---|---|
| `pay-as-you-go` | openai-chat, anthropic | `https://api.xiaomimimo.com/v1` · `/anthropic` | `mimo-v2.5-pro`, `mimo-v2.5`, `mimo-v2-pro`, `mimo-v2-omni`, `mimo-v2-flash` |
| `token-plan-cn` | openai-chat, anthropic | `https://token-plan-cn.xiaomimimo.com/v1` · `/anthropic` | same family |

- `mimo-v2.5-pro` is the recommended coding default (1M context, 128K output, web search, function calls); `v2-omni` adds full-modal input.
- Token Plan Singapore/Amsterdam regional endpoints exist upstream but are not in the catalog — custom profile if needed.

## Volcengine / ModelArk (`volcengine`)

| Endpoint group | Dialects | Base URL | Key models |
|---|---|---|---|
| `ark-api` | openai-chat, openai-responses, anthropic | `https://ark.cn-beijing.volces.com/api/v3` · `/api/compatible` | versioned deployments: `doubao-seed-2-0-code-preview-260215`, `…-pro-260215`, `…-lite-260428`, `deepseek-v4-pro-260425` |
| `coding-plan` | openai-chat, anthropic | `…/api/coding/v3` · `/api/coding` | plan aliases: `ark-code-latest`, `doubao-seed-2.0-code/pro`, `minimax-m3`, `glm-5.2`, `deepseek-v4-pro`, `kimi-k2.6` |
| `agent-plan` | openai-chat, anthropic | `…/api/plan/v3` · `/api/plan` | same alias set |

- Ark API uses **versioned deployment ids**; the plans use rolling aliases (`ark-code-latest`) — the two id styles are not interchangeable across endpoint groups.
- Plans expose flagship third-party models (MiniMax, GLM, DeepSeek, Kimi) under one Volcengine credential.

## Z.AI / GLM (`zai`)

| Endpoint group | Dialects | Base URL | Key models |
|---|---|---|---|
| `global` | openai-chat | `https://api.z.ai/api/paas/v4` | `glm-5.2`, `glm-5-turbo`, `glm-5v-turbo`, `glm-5.1`, `glm-4.7`, `glm-4.5-air` |
| `cn` | openai-chat | `https://open.bigmodel.cn/api/paas/v4` | same family |
| `coding-global` | openai-chat, anthropic | `…/api/coding/paas/v4` · `https://api.z.ai/api/anthropic` | `glm-5.2`, `glm-5-turbo`, `glm-4.7`, `glm-4.5-air` |
| `coding-cn` | openai-chat, anthropic | `…/api/coding/paas/v4` · `https://open.bigmodel.cn/api/anthropic` | same set |

- The Anthropic-compatible endpoints belong to the **Coding Plan product**, not the general API — a general API key will not authenticate there.

## Google Gemini / Vertex AI (`gemini`)

| Endpoint group | Dialects | Base URL | Key models |
|---|---|---|---|
| `gemini-api` | gemini, openai-chat | `https://generativelanguage.googleapis.com` (+`/v1beta/openai`) | `gemini-3.1-pro-preview`, `gemini-3-flash-preview`, `gemini-2.5-pro/flash(-lite)`, `gemini-3.1-flash-lite` |
| `google-accounts` | gemini | `https://cloudcode-pa.googleapis.com` | same family |
| `vertex-openai-compatible` | openai-chat | user-supplied | `google/…`-prefixed ids |

- Three auth shapes: `gemini-api` = API key; `google-accounts` = Google OAuth (code-assist flow, handled by a dedicated adapter); Vertex = your own endpoint URL with `google/`-prefixed model ids.

## xAI / Grok (`xai`)

| Endpoint group | Dialects | Base URL | Key models |
|---|---|---|---|
| default | openai-chat, openai-responses | `https://api.x.ai/v1` | `grok-4.3`, `grok-build-0.1` |

## NVIDIA NIM (`nvidia`)

| Endpoint group | Dialects | Base URL | Key models |
|---|---|---|---|
| default | openai-chat | `https://integrate.api.nvidia.com/v1` | `nvidia/nemotron-3-super-120b-a12b`, `nvidia/nemotron-3-nano-30b-a3b`, `qwen/qwen3-coder-480b`, `openai/gpt-oss-120b`, `moonshotai/kimi-k2.6` |

## OpenRouter (`openrouter`)

| Endpoint group | Dialects | Base URL | Key models |
|---|---|---|---|
| default | openai-chat | `https://openrouter.ai/api/v1` | `anthropic/claude-sonnet-4.5`, `openai/gpt-4o`, `deepseek/deepseek-chat`, `google/gemini-2.5-pro` |

- Catalog carries four representative ids; OpenRouter's whole marketplace works — type any model id the router accepts.

## Azure OpenAI (`azure`)

| Endpoint group | Dialects | Base URL | Key models |
|---|---|---|---|
| default | openai-responses | user-supplied resource URL | user-supplied deployment name |

- Azure endpoints are per-resource (`https://{resource}.openai.azure.com/openai/v1/`) and models are your deployment names — the catalog intentionally ships both fields empty.

---

**Provenance.** Base URLs and model sets mirror `src/resources/profile-catalog/` (the shipped catalog); plan/credential semantics were verified against official provider documentation in the 2026-06-18 audits (`reports/provider-plan-api-audit-2026-06-18.md`, `reports/provider-profile-official-verification-2026-06-18.md`). The catalog deliberately has no model-discovery metadata — model lists are curated, and a custom profile covers anything beyond them.

*Source anchors: `src/resources/profile-catalog/*.json` (endpoints, base URLs, models), `src/core/src/profiles/catalog.rs` (loader), `reports/provider-plan-api-audit-2026-06-18.md` + `reports/provider-profile-official-verification-2026-06-18.md` (official-doc verification).*
*Last verified: v0.7.11*

<sub>[◀ Timers and limits](timers-and-limits.md) · [Documentation index](../README.md) · [Internals ▶](../internals/README.md)</sub>
