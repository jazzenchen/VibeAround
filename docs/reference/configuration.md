# Configuration reference

Every file VibeAround reads or writes, with full schemas for the ones you may edit by hand. Companion pages: [CLI reference](cli.md), [API surfaces](api-surfaces.md), [provider endpoints](provider-endpoints.md).

## All files on disk

Everything lives under `~/.vibearound/` (override with `VIBEAROUND_DATA_DIR`):

| File / directory | Written by | Contents | Hand-editable? |
|---|---|---|---|
| `settings.json` | you, desktop settings UI, onboarding | Main configuration — [full schema below](#settingsjson) | **Yes** (then `va settings reload`) |
| `agents.json` | desktop Launch UI, `va-launch` (executable discovery) | Per-agent launch preferences — [schema below](#agentsjson) | Yes, carefully |
| `launch/profiles/<name>.json` | you, desktop (temp materialized copies) | Saved native-launch profiles — [schema below](#launch-profile-json-schema-v1) | **Yes** (that is the point) |
| `auth.json` | daemon, every start | `{port, token, mcp_token, bridge_token, agent_token}` — one credential per route family: `token` for dashboard/control, `mcp_token` for `/mcp`, `bridge_token` for `/local-api`, `agent_token` for `/local-agent` | No — rewritten each start, except `agent_token`, which is carried over |
| `profiles/<profile-id>.json` | desktop/dashboard profile UI | Saved model profiles (provider, endpoint, key, model routes) | Prefer the UI; hand-edits are read on reload |
| `profile-state/<profile-id>/` | profile rendering | Rendered per-profile agent config files (settings overlays); env pointers reference these ([launch internals](../internals/launch.md#environment-assembly-layer-by-layer)) | No — regenerated per render |
| `plugins/<kind>/` | desktop plugin manager | Installed channel plugins + manifests | Only during plugin development |
| `workspaces/` | daemon | Default root for created workspaces | It is your files |
| `.cache/` | channel plugins | Downloaded chat attachments | Safe to purge |
| `logs/runtime/` | daemon | Daily rolling log files (`vibearound.log.<date>`) | Safe to purge |
| `*.jsonl` (workspace/thread/attachment event logs) | daemon | Conversation state ([workspace module](../internals/modules/workspace.md)) | **No** — append-only event logs |
| `desktop-apps.detected.json` | desktop detection | Cached Claude/Codex Desktop app locations | No — cache |

Before each launch, VibeAround replaces its reserved project skills and writes the current daemon MCP credential into the project's agent config. Paths come from the agent registry; unrelated configuration is unchanged ([launch flow, step 5](../internals/flows/native-launch.md)).

## settings.json

Location: `~/.vibearound/settings.json`. Created with defaults on first run; apply edits with `va settings reload`, the desktop reload action, or a daemon restart. Unknown keys are ignored.

```jsonc
{
  // --- Tunnel (see ../guides/tunnels-and-remote-access.md) ---
  "tunnel": {
    "provider": "none",              // none | ngrok | localtunnel | cloudflare | tailscale
    "ngrok":      { "auth_token": "…", "domain": "…" },
    "cloudflare": { "tunnel_token": "…", "hostname": "…" }
  },

  // --- Toolchain ---
  "toolchain_mode": "system",        // system | managed

  // --- Workspaces ---
  "default_workspace": "~/…",        // root for new agent sessions
  "workspaces": ["~/dev/app-a"],     // additional registered project folders

  // --- Agents ---
  "default_agent": "claude",
  "enabled_agents": ["claude", "codex"],  // omit to enable every known agent

  // --- Networking ---
  "proxy": { "enabled": true, "http_proxy": "http://…", "no_proxy": "…" },

  // --- Bridge behavior (see ../architecture/local-api-and-bridge.md) ---
  "api_bridge": {
    "replace_provider_web_search": false
  },
  // Service switch + per-agent opt-in: both must be on for an agent to
  // serve /local-agent routes (an agent not listed answers 403). The
  // direct profile is always refused (429) — use a managed profile.
  "local_agent_api": { "enabled": true, "agents": ["claude"] },

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
  // Per-channel fields: see guides/channels/. Every channel also accepts
  // an optional verbose object (both flags default to false).
  "channels": {
    "telegram": { "bot_token": "…", "verbose": { "show_thinking": true, "show_tool_use": true } },
    "feishu":   { "app_id": "…", "app_secret": "…" }
  },

  // --- Web terminal ---
  "tmux": { "detach_others": true }
}
```

## agents.json

Launch preferences, three layers deep. Agent ids accept the registry aliases ([supported matrix](../product/supported-matrix.md)).

```jsonc
{
  "selected_agent": "claude",        // Launch tab's visible agent (UI state)
  "default_agent": "claude",         // VibeAround-wide default: tray quick launch, IM thread creation
  "default_profile_id": "moonshot",  // profile snapshot for that default
  "agents": {
    "codex": {
      "profile_id": "deepseek",      // per-agent default profile
      "workspace": "~/dev/app",      // per-agent default workspace
      "executable": {                 // resolved CLI — written back by va-launch discovery
        "path": "/opt/homebrew/bin/codex",
        "version": "…", "source": "path-scan", "rank": 0
      },
      "launch_args": {
        "terminal": ["--flag-for-your-own-terminal"],  // native launches only
        "acp": ["--flag-for-hosted-spawns"]            // IM/web hosted spawns only
      }
    }
  }
}
```

The two `launch_args` lists are deliberately separate — a flag you trust in your own terminal is not automatically safe for an IM-driven host ([launch internals](../internals/launch.md#argument-handling)). A stale `executable.path` makes launches fail validation; delete the entry to force a PATH re-scan.

## Launch profile JSON (schema v1)

Saved under `launch/profiles/<name>.json`, consumed by `va launch --profile <name>` / `--profile-path <file>`. **Unknown fields are rejected** — handing the launcher a provider profile or other JSON fails loudly instead of half-working.

```jsonc
{
  "schemaVersion": 1,
  "id": "openai-codex",              // profile name
  "agent": "codex",                  // registry agent id
  "profileId": "openai",             // metadata only — va-launch never reads provider storage
  "launchTarget": "codex",
  "workspace": "/Users/example/project",
  "terminal": "terminal",            // terminal id; see launch internals for the per-OS list
  "command": "codex",                // command line (quote-aware word splitting)
  "executablePath": null,            // explicit CLI override (skips agents.json + PATH)
  "windowsExecutablePath": null,     // Windows app-launch variant
  "windowLabel": "OpenAI Codex",
  "env": { "OPENAI_API_KEY": "…" },  // exported by the generated launch script
  "args": { "native": ["--model", "gpt-5"] },
  "cleanupPaths": [],                // temp files deleted after the command exits
  "macosAppProbe": null,             // app name for the "already running" osascript check
  "windowsProcessProbe": null
}
```

Two "profile" concepts meet here and must not be confused: a **provider profile** (credentials + model routing, managed in the app) versus a **launch profile** (this file — a native launch request). A resolver connects them: the desktop renders a provider profile *into* a materialized launch profile at launch time; saved CLI launch profiles hold the rendered snapshot ([launch internals](../internals/launch.md#producers-desktop-vs-cli)).

## Environment variables

| Variable | Consumer | Meaning |
|---|---|---|
| `VIBEAROUND_DATA_DIR` | daemon, va-launch | Override `~/.vibearound` |
| `RUST_LOG` | daemon | Log filter (default `info,common=debug`); see [troubleshooting](../guides/troubleshooting-and-faq.md#where-are-the-logs) |
| `VIBEAROUND_VA_LAUNCH_BIN` | desktop/CLI (dev only) | Point at a non-packaged `va-launch` |
| `VIBEAROUND_CHANNEL_KIND`, `VIBEAROUND_CHAT_ID`, `VIBEAROUND_AGENT_KIND`, `VIBEAROUND_THREAD_ID`, `VIBEAROUND_WORKSPACE_ID` | hosted agent processes | Injected context about the owning route/thread |

## Data directory

```text
~/.vibearound/
├── settings.json           # configuration (this page)
├── auth.json               # port + dashboard, MCP, bridge and agent-as-API tokens
├── agents.json             # resolved agent executables (va-launch cache)
├── plugins/<kind>/         # installed channel plugins
├── launch/profiles/        # saved launch profile JSON (schema v1)
├── workspaces/             # default root for created workspaces
├── .cache/                 # channel attachment cache
└── workspace-threads.jsonl # + workspace/attachment event logs
```

Default port: `12358`. Dashboard: `http://127.0.0.1:12358/va/` (token required; the root path redirects to `/va/`).

---

*Source anchors: `src/core/src/config.rs` (settings parser — key names above mirror it), `src/core/src/workspace/threads/runtime.rs` (injected env), `src/launcher/` (agents.json, launch profiles).*
*Last verified: v0.7.11*

<sub>[◀ Security model](../architecture/security-model.md) · [Documentation index](../README.md) · [CLI reference ▶](cli.md)</sub>
