# Local API and bridge

VibeAround ships its own model API bridge: a local translation layer that lets any supported client API dialect talk to any configured provider. It is what makes "one Kimi subscription powers Codex, Claude Code, and Gemini CLI" work. This page explains what the bridge is and how requests flow through it; for setting up profiles see the [Model profiles guide](model-profiles-guide.md).

## What problem the bridge solves

Agent CLIs are hardwired to a vendor dialect: Codex speaks the OpenAI Responses API, Claude Code speaks Anthropic Messages, Gemini CLI speaks Gemini GenerateContent. Model providers, meanwhile, each expose one dialect of their own. Without translation, your provider choice dictates your agent choice.

The bridge decouples them. It accepts requests in the **client's** dialect on a local endpoint, translates to a universal internal representation, then encodes for the **provider's** dialect upstream — streaming included, in both directions.

## Supported protocols

Four dialects are translated on both sides:

| API type id | Dialect |
|---|---|
| `openai-responses` | OpenAI Responses API |
| `openai-chat` | OpenAI Chat Completions |
| `anthropic` | Anthropic Messages |
| `gemini` | Gemini GenerateContent |

Any client dialect can be paired with any provider dialect. When client and provider dialects match, the bridge can pass requests through with minimal touching.

## Endpoint families

All bridge endpoints are served by the local daemon under `/va/` and accept only local, authenticated callers:

```text
/va/local-api/{profile}/{scope}/{target_api_type}/v1/…    # profile-scoped bridge (primary)
/va/local-agent/{agent_id}/{profile_id}/v1/…               # agent-as-API
/va/bridge/{profile_id}/{target_api_type}/v1/…             # legacy shape
```

Each family exposes the standard sub-paths clients expect: `/v1/chat/completions`, `/v1/responses`, `/v1/messages`, `/v1/models`, and Gemini's generateContent route.

**Profile-scoped bridge (`local-api`).** The main path. `{profile}` selects the credential and provider, `{scope}` identifies which launch/client the URL was minted for, `{target_api_type}` declares the client's dialect. When you launch an agent through a bridged profile, VibeAround renders these URLs directly into the agent's config — Codex gets a `base_url` under `/openai-responses/v1`, Claude Code gets an Anthropic-shaped endpoint, and neither knows the real provider.

**Agent-as-API (`local-agent`).** Turns a hosted coding agent itself into an OpenAI/Anthropic-compatible endpoint: requests become prompts to a real agent (with its tools and workspace), responses stream back in the requested dialect. This lets any OpenAI-compatible tool drive a full coding agent.

## What happens to a request

1. **Decode** from the client dialect into the universal request.
2. **Model mapping.** The requested model id is mapped through the profile's model routes — including "fake" model ids that agents insist on (an agent that only accepts `gpt-*` names can be given an alias that maps to `kimi-k2.7-code` upstream).
3. **Content policy.** Request content is sanitized against the upstream model's declared capabilities — for example, image parts are stripped or rejected when the target model has no image input.
4. **Web search handling.** Provider-native web search tools can be replaced by VibeAround's host-side search (`replace_provider_web_search`), so models without native search still get real results, and all agents share one search configuration.
5. **Encode and send** upstream with the profile's credentials; rate-limit responses get bounded retry.
6. **Translate the response** (or stream) back into the client dialect. A live recorder can capture request/response bodies for the launch popup's debugging view — in memory only, never persisted.

## Provider notes

Provider specifics live in the profile catalog (Moonshot/Kimi, DashScope/Qwen, DeepSeek, OpenRouter, MiniMax, MiMo, Volcengine, Z.AI/GLM, Gemini, xAI, NVIDIA, Azure OpenAI). Catalog entries declare endpoints, dialects, models, context windows, and capabilities, so new provider models are a catalog update rather than a code change. DeepSeek and several others have dedicated translation quirks handled by provider adapters.

Authentication to upstream providers is static API key per profile. OAuth/subscription-based upstream auth (for example Gemini's code-assist flow) is handled by a dedicated adapter where supported.

## Trust boundary

The bridge binds to the loopback interface and requires the local bridge auth gate; it is not reachable through tunnels. Request bodies up to 64 MB are accepted to accommodate large context payloads. Credentials never appear in rendered client configs — only local URLs do; the daemon injects real keys upstream.

---

*Source anchors: `src/server/src/web_server/api_bridge/` (protocol, routes, model_mapping, content_policy, upstream, local_agent), `src/core/src/profiles/bridge_launch.rs` (URL rendering), `src/resources/profile-catalog/` (providers), `src/server/src/web_server/mod.rs` (route table, body limit).*
*Last verified: v0.7.11*
