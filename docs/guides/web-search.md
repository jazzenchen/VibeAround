# Host web search

Models without native web search can still search through VibeAround's host-side search tool. Enable `search_tool` in settings, then optionally turn on `api_bridge.replace_provider_web_search` to route even native-search-capable models through your configured search sources. One search config can serve every agent and model profile.

## Configuration

In `~/.vibearound/settings.json`:

```jsonc
{
  "search_tool": {
    "enabled": true,
    "max_results": 5,
    "sources": {
      "tavily": { "enabled": true, "api_key": "..." },
      "brave": { "enabled": false, "api_key_env": "BRAVE_KEY", "base_url": null },
      "exa": { "enabled": false, "api_key_env": "EXA_API_KEY" },
      "grok": { "enabled": false, "api_key_env": "XAI_API_KEY" }
    }
  },
  "api_bridge": {
    "replace_provider_web_search": false
  }
}
```

Supported sources are Tavily, Brave, Exa, and Grok. Keys can be stored directly in `api_key` or loaded from an environment variable with `api_key_env`.

## When to replace provider web search

Leave `replace_provider_web_search` off when you want providers with native search support to use their own implementation.

Turn it on when you need one consistent search source across agents, or when a provider advertises `web_search` but the selected model, endpoint, or account plan does not actually allow it.

## Related pages

- [Model profiles guide](model-profiles.md)
- [Provider endpoints reference](../reference/provider-endpoints.md)
- [Local API and bridge](../architecture/local-api-and-bridge.md)

---

*Source anchors: `src/core/src/config.rs` (search_tool and api_bridge settings), `src/server/src/web_server/api_bridge/` (provider tool translation).*
*Last verified: v0.7.12*

<sub>[◀ Model profiles guide](model-profiles.md) · [Documentation index](../README.md) · [Connect channels ▶](connect-channels.md)</sub>
