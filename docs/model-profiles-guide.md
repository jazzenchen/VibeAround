# Model profiles guide

A model profile stores a provider credential plus routing rules, so agents can use that provider without knowing anything about it. This page shows how to create and use profiles; the mechanism behind them is explained in [Local API and bridge](local-api-and-bridge.md).

## When you need a profile — and when you don't

- **No profile needed** (`direct`): each agent uses its own vendor login (Claude Code with your Anthropic account, Codex with OpenAI). VibeAround hosts the agent but stays out of the model path.
- **Profile needed**: you want a third-party or self-chosen provider — Kimi for Codex, DeepSeek for Claude Code, one subscription across all your agents, or a custom OpenAI-compatible endpoint.

## Creating a profile

In the desktop app's model profile screen (or via the dashboard):

1. **Pick a provider.** The catalog offers Kimi/Moonshot, DashScope, DeepSeek, OpenRouter, MiniMax, MiMo, Volcengine, Z.AI/GLM, Gemini/Vertex, xAI, NVIDIA NIM, and Azure OpenAI — each with known endpoints, models, and context windows prefilled. Or choose **custom** for any compatible endpoint.
2. **Pick the endpoint variant** where the provider has several (global vs CN, pay-as-you-go vs coding plan). Variants differ in base URL, available models, and sometimes dialect.
3. **Paste the API key.** Keys are stored locally and only ever sent to that provider by the daemon.
4. **Choose models.** Which upstream models the profile exposes, and which is the default. For agents that validate model names, the profile can define alias model ids that map to the real upstream model.

Profiles are managed at runtime — create, edit, reorder, delete — without restarting the daemon. `va profiles` lists them from the CLI.

## Using a profile

**Hosted conversations (IM / web chat).** Bind a profile per channel default (`remote.channels.<kind>.profile_id`), per thread (`/switch host <agent> <profile>` or `/profile --switch <id>`), or per web-chat launch selection. The hosted agent process gets environment and config pointing at the local bridge under that profile.

**Terminal launches.** Pick agent + profile in the desktop Launch screen or a saved launch profile (`va launch --profile <name>`). The rendered config makes the agent talk to `http://127.0.0.1:12358/va/local-api/...`; the daemon translates to the provider. See [Agent launch guide](agent-launch-guide.md).

**Any OpenAI-compatible tool.** Point the tool at the profile's local endpoint (shown in the profile UI) and it gets the same translation — this is how non-agent tools share your provider setup.

## Choosing the API type pairing

Each profile endpoint declares its upstream dialect; each agent speaks its own client dialect. The bridge translates any pairing, but two rules of thumb:

- **Match dialects when possible** (e.g. Kimi's `anthropic` endpoint for Claude Code) — passthrough is more faithful than translation, especially for provider-specific features.
- **Let the catalog decide otherwise.** Catalog entries mark which dialect each endpoint serves best; the launch flow picks a sensible route (`bridge_route_preference`) unless you override it.

Model capabilities matter too: the bridge sanitizes requests against the target model's declared capabilities (for example dropping image parts for text-only models), so a mismatch degrades gracefully rather than erroring — but picking a model with the capabilities your agent uses (image input, long context) avoids surprises.

## Web search replacement

Models without native web search can still search: enable the host-side search tool (`search_tool` in settings) and, optionally, `api_bridge.replace_provider_web_search` to route even native-search-capable models through your configured search sources. One search config then serves every agent and profile.

## Troubleshooting profiles

| Symptom | Likely cause |
|---|---|
| Agent says model not found | The agent validates model ids — use the profile's alias model id, not the upstream id |
| 401 from provider | Key invalid/expired, or wrong endpoint variant (global key on CN endpoint) |
| Replies work but images ignored | Target model lacks image input; check the model's capabilities in the profile |
| Rate limit errors under load | The bridge retries with backoff automatically; persistent 429s mean the plan's quota is the limit |
| Agent launched but hits `ConnectionRefused` | The daemon is not running — bridged profiles need it alive; start the desktop app or `va serve` |

---

*Source anchors: `src/resources/profile-catalog/` (providers, endpoints, models, capabilities), `src/core/src/profiles/` (schema, catalog, render, bridge_launch), `src/server/src/web_server/api_bridge/` (model_mapping, content_policy, rate-limit retry), `src/core/src/config.rs` (search_tool, replace_provider_web_search).*
*Last verified: v0.7.11*
