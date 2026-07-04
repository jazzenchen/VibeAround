# Configuration reference

settings.json, environment variables, and the data directory layout. Companion pages: [CLI reference](cli.md), [API surfaces](api-surfaces.md).

## settings.json

Location: `~/.vibearound/settings.json`. Created with defaults on first run; apply edits with `va settings reload`, the desktop reload action, or a daemon restart. Unknown keys are ignored.

```jsonc
{
  // --- Tunnel (see ../guides/tunnels-and-remote-access.md) ---
  "tunnel": {
    "provider": "none",              // none | ngrok | localtunnel | cloudflare
    "ngrok":      { "auth_token": "…", "domain": "…" },
    "cloudflare": { "tunnel_token": "…", "hostname": "…" }
  },
  "preview_base_url": null,          // override the public base URL for preview links

  // --- Toolchain ---
  "toolchain_mode": "system",        // system | managed

  // --- Workspaces ---
  "default_workspace": "~/…",        // root for new agent sessions
  "workspaces": ["~/dev/app-a"],     // additional registered project folders

  // --- Agents ---
  "default_agent": "claude",
  "enabled_agents": ["claude", "codex"],  // omit to enable every known agent
  "integrations": {
    "mcp_auto_install": true,        // write VibeAround MCP config into agent configs
    "skill_auto_install": true      // write VibeAround skills into agent skill dirs
  },

  // --- Networking ---
  "proxy": { "enabled": true, "http_proxy": "http://…", "no_proxy": "…" },

  // --- Bridge behavior (see ../architecture/local-api-and-bridge.md) ---
  "api_bridge": {
    "replace_provider_web_search": false
  },
  "local_agent_api": { "enabled": true },

  // --- Host-side web search ---
  "search_tool": {
    "enabled": false,
    "max_results": 5,
    "sources": {
      "tavily": { "enabled": true, "api_key": "…" },   // also: brave, exa, grok
      "brave":  { "enabled": false, "api_key_env": "BRAVE_KEY", "base_url": null }
    }
  },

  // --- Per-channel defaults (see ../guides/connect-channels.md) ---
  "remote": {
    "channels": { "telegram": { "agent_id": "claude", "profile_id": "moonshot" } }
  },

  // --- Channel plugin config: passed to plugins verbatim ---
  "channels": {
    "telegram": { "bot_token": "…" },
    "feishu":   { "app_id": "…", "app_secret": "…" }
  },

  // --- Web terminal ---
  "tmux": { "detach_others": true }
}
```

## Environment variables

| Variable | Consumer | Meaning |
|---|---|---|
| `VIBEAROUND_DATA_DIR` | daemon, va-launch | Override `~/.vibearound` |
| `VIBEAROUND_VA_LAUNCH_BIN` | desktop/CLI (dev only) | Point at a non-packaged `va-launch` |
| `VIBEAROUND_CHANNEL_KIND`, `VIBEAROUND_CHAT_ID`, `VIBEAROUND_AGENT_KIND`, `VIBEAROUND_THREAD_ID`, `VIBEAROUND_WORKSPACE_ID` | hosted agent processes | Injected context about the owning route/thread |

## Data directory

```text
~/.vibearound/
├── settings.json           # configuration (this page)
├── auth.json               # dashboard token, rewritten each daemon start
├── agents.json             # resolved agent executables (va-launch cache)
├── plugins/<kind>/         # installed channel plugins
├── launch/profiles/        # saved launch profile JSON (schema v1)
├── workspaces/             # default root for created workspaces
├── .cache/                 # channel attachment cache
└── workspace-threads.jsonl # + workspace/attachment event logs
```

Default port: `12358`. Dashboard: `http://127.0.0.1:12358/` (token required).

---

*Source anchors: `src/core/src/config.rs` (settings parser — key names above mirror it), `src/core/src/workspace/threads/runtime.rs` (injected env), `src/launcher/` (agents.json, launch profiles).*
*Last verified: v0.7.11*

<sub>[◀ Security model](../architecture/security-model.md) · [Documentation index](../README.md) · [CLI reference ▶](cli.md)</sub>
