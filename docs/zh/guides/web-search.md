# 宿主网页搜索

没有原生联网搜索的模型，也可以通过 VibeAround 的宿主侧搜索工具完成搜索。在 settings 里启用 `search_tool`，并可选开启 `api_bridge.replace_provider_web_search`，让具备原生搜索能力的模型也统一走你配置的搜索源。一份搜索配置可以服务所有 Agent 和模型 Profile。

## 配置

在 `~/.vibearound/settings.json`：

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

支持的搜索源包括 Tavily、Brave、Exa 和 Grok。Key 可以直接写在 `api_key`，也可以通过 `api_key_env` 从环境变量读取。

## 什么时候替换供应商搜索

当你希望支持原生搜索的供应商继续使用自己的实现时，保持 `replace_provider_web_search` 关闭。

当你需要所有 Agent 使用同一套搜索源，或供应商声明支持 `web_search` 但当前模型、端点、账号套餐实际不可用时，再开启它。

## 相关页面

- [模型 Profile 指南](model-profiles.md)
- [供应商端点参考](../reference/provider-endpoints.md)
- [本地 API 与 Bridge](../architecture/local-api-and-bridge.md)

---

*Source anchors: `src/core/src/config.rs` (search_tool and api_bridge settings), `src/server/src/web_server/api_bridge/` (provider tool translation).*
*Last verified: v0.7.12*

<sub>[◀ 模型 Profile 指南](model-profiles.md) · [文档索引](../README.md) · [连接渠道 ▶](connect-channels.md)</sub>
