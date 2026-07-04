# Flow: bridge request

One model API call from a launched agent CLI, followed through the local bridge to the provider and back. Concepts in [Local API and bridge](../../architecture/local-api-and-bridge.md); this page is the request path itself.

## Hop by hop

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

**1. The CLI calls localhost.** A launch through a bridged profile rendered base URLs like `http://127.0.0.1:12358/va/local-api/moonshot/codex-openai-responses/openai-responses/v1` into the agent's config. The CLI thinks this *is* its vendor API. The local-bridge gate rejects non-loopback callers; bodies up to 64 MB are accepted.
→ `src/core/src/profiles/bridge_launch.rs` (URL rendering), `src/server/src/web_server/auth.rs` (`require_local_bridge`)

**2. Decode.** `{api_type}` names the client dialect; the matching translator (OpenAI Responses / OpenAI Chat / Anthropic Messages / Gemini) decodes into a universal request. When the client and provider dialects match and no rewriting is needed, the bridge can take a passthrough path instead.
→ `src/server/src/web_server/api_bridge/protocol.rs`, `passthrough.rs`

**3. Model mapping.** The requested model id runs through the profile's model routes: alias ids (minted for CLIs that validate model names against vendor lists) map to real upstream models; the route preference also picks which of the profile's endpoints/dialects to use.
→ `api_bridge/model_mapping.rs`

**4. Content policy.** The request is sanitized against the target model's declared capabilities from the catalog — e.g. image parts stripped for text-only models — so mismatches degrade instead of erroring upstream.
→ `api_bridge/content_policy.rs`, `src/resources/profile-catalog/` (capabilities)

**5. Web search handling.** If the request carries a provider-native web-search tool and host-side search is configured (or `replace_provider_web_search` forces it), the bridge swaps in VibeAround's search tool so results come from your configured sources.
→ `api_bridge/server_tools.rs`, `src/core/src/search.rs`

**6. Upstream.** The universal request is encoded in the provider's dialect, the profile's real API key is attached (it exists only daemon-side — never in the CLI's config), and the call is made with bounded retry on rate-limit responses. Provider quirks (DeepSeek et al.) are handled by adapters; Gemini's OAuth code-assist path has its own adapter.
→ `api_bridge/upstream.rs`, `google_code_assist.rs`, the `va-ai-api-bridge` crate (translators)

**7. Stream back.** Provider stream events are translated to the client dialect's streaming format chunk by chunk (or buffered for non-streaming calls). If the launch popup's recorder is active, request/response bodies are captured in memory for debugging — nothing persists.
→ `api_bridge/stream.rs`, `completion.rs`, `src/server/src/web_server/bridge_recording.rs`

## The agent-as-API variant

`/va/local-agent/{agent}/{profile}/v1/…` runs the same decode step, but instead of an upstream provider the universal request becomes a **prompt to a hosted agent process** — tools, workspace and all — and the agent's output is encoded back as a chat/responses-style reply. Same gate, same dialects, different executor.
→ `api_bridge/local_agent.rs`

## Failure behavior

| Failure | Result |
|---|---|
| Daemon not running | CLI sees connection refused — bridged profiles need the daemon alive |
| Unknown profile / api_type | 4xx JSON error in the client's dialect |
| Upstream 429 | Bounded retry with backoff, then the error is translated back |
| Upstream auth failure | Translated 401 — check the profile's key, not the CLI |
| Model lacks a requested capability | Sanitized request (degraded), not an upstream rejection |

---

*Source anchors: `src/server/src/web_server/api_bridge/` (protocol, routes, model_mapping, content_policy, server_tools, upstream, stream, completion, local_agent), `src/core/src/profiles/bridge_launch.rs`, `src/server/src/web_server/bridge_recording.rs`.*
*Last verified: v0.7.11*

<sub>[◀ Flow: permission request](permission.md) · [Documentation index](../../README.md) · [Flow: agent launch ▶](native-launch.md)</sub>
