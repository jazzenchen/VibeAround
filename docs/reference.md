# Reference

Lookup tables: settings.json, the `va` CLI, MCP tools, local API routes, environment variables, and the data directory. Task-oriented guidance lives in the other usage pages.

## settings.json

Location: `~/.vibearound/settings.json`. Created with defaults on first run; apply edits with `va settings reload`, the desktop reload action, or a daemon restart. Unknown keys are ignored.

```jsonc
{
  // --- Tunnel (see tunnels-and-remote-access.md) ---
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

  // --- Bridge behavior (see local-api-and-bridge.md) ---
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

  // --- Per-channel defaults (see connect-channels.md) ---
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

## `va` CLI

Global flags: `--auth-file PATH`, `--base-url URL`, `--token TOKEN`, `--json`.

| Command | Purpose |
|---|---|
| `va help` | Show usage |
| `va health` / `va info` / `va status` / `va doctor` | Liveness / metadata / runtime summary / diagnosis |
| `va serve` | Start the standalone server |
| `va auth status` / `va auth clear` | Show / remove saved auth |
| `va pair start [--wait --save]` / `va pair status SID [--save]` / `va pair wait SID [--save]` | Pairing flows |
| `va chat send TEXT` (`--stdin`, `--continue`) / `va chat repl` / `va chat sessions` / `va chat forget [--all]` | Chat from the terminal over `/ws/chat` |
| `va channels` / `va channel sync` / `va channel start\|stop\|restart KIND` | Channel plugin lifecycle |
| `va tunnels` / `va tunnel kill PROVIDER` | Tunnel runtimes |
| `va agents` / `va agent kill ROUTE_KEY` | Enabled agents / kill an attached runtime |
| `va launch --profile NAME` / `--profile-path PATH` (`--dry-run`) | Native agent launch |
| `va launch sessions` / `va launch archive\|unarchive --agent A ID` | Resumable native sessions |
| `va sessions` / `va session create --tool TOOL [--attach]` / `va session attach ID` / `va session kill ID` / `va pty kill ID` | PTY sessions |
| `va tmux sessions` | Attachable tmux sessions |
| `va workspaces` / `va workspace add\|remove\|default PATH` / `va workspace create NAME` | Workspace registry |
| `va previews` / `va preview delete SLUG` | Live previews |
| `va profiles` | List model profiles |
| `va settings reload` | Re-read settings.json |

`vibearound` is a full alias; `vibearound tui` opens the TUI.

## MCP tools

The daemon serves MCP at `/mcp` (token-authenticated; auto-injected into enabled agents' configs). Tools:

| Tool | Purpose |
|---|---|
| `get_session_id` | Resolve the calling agent session's identity |
| `prepare_handover` | Issue a pickup code for cross-surface continuity |
| `register_workspace` | Register the current project as a workspace |
| `initialize_subagents` | Start a multi-agent turn (parallel / collaboration / brainstorming) |
| `wait_for_subagents` | Block until subagents report completion |
| `preview` | Create a live preview for a dev server port |
| `md_preview` | Create a rendered Markdown preview |

The corresponding agent skills (`vibearound`, `va-session`, `va-preview`, `va-md-preview`, `agent-collaboration`) are installed per agent when `skill_auto_install` is on.

## Local API route families

Loopback-only, local-bridge-gated; details in [Local API and bridge](local-api-and-bridge.md).

```text
/va/local-api/{profile}/{scope}/{api_type}/v1/{responses|chat/completions|messages|models}
/va/local-agent/{agent}/{profile}/v1/{responses|chat/completions|messages|models}
/va/bridge/{profile}/{api_type}/v1/…          (legacy)
```

`{api_type}` ∈ `openai-responses` | `openai-chat` | `anthropic` | `gemini`.

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

*Source anchors: `src/core/src/config.rs` (settings parser — key names above mirror it), `src/cli/src/args.rs` (usage), `src/server/src/web_server/mcp/mod.rs` (tool dispatch), `src/server/src/web_server/mod.rs` (routes), `src/core/src/workspace/threads/runtime.rs` (injected env), `docs internal: va-launch notes` (launch profile schema).*
*Last verified: v0.7.11*
