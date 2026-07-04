# Agent launch guide

Agent Launch opens a coding agent CLI in your own terminal — credentials materialized, model routing configured, project integrations installed — instead of hosting it inside the daemon. Use it when you want the full native CLI experience with VibeAround's profile and continuity benefits.

## Launching from the desktop app

The Launch screen asks for three choices:

1. **Agent** — any enabled agent, including the desktop-app targets (`claude-desktop`, `codex-desktop`) which open the vendor's GUI app instead of a CLI.
2. **Workspace** — the directory the agent starts in.
3. **Model profile** — `direct` (agent's own vendor login) or one of your provider profiles (see [Model profiles guide](model-profiles.md)).

VibeAround renders the launch — environment variables, per-agent config overlays, bridge URLs when the profile needs them — and opens your terminal app (Terminal.app or iTerm2 on macOS, PowerShell on Windows, `xdg-terminal-exec`/common terminals on Linux). Terminal preference is configurable.

## Launching from the CLI

Saved launch profiles live in `~/.vibearound/launch/profiles/<name>.json`:

```bash
va launch --profile codex           # by name
va launch --profile-path ./my.json  # by file
va launch --profile codex --dry-run # validate and print the plan, launch nothing
```

`va launch` execs the bundled native `va-launch` binary — a standalone launcher that also works by itself (`/path/to/va-launch --profile <name>`), so scripted and CI-adjacent launches do not need the full CLI.

A launch profile is a small JSON document (schema version 1): agent, workspace, terminal choice, command/executable override, env, args, window label. Unknown fields are rejected to catch handing it the wrong kind of JSON.

## What happens at launch

1. **Validation.** Workspace exists, agent executable resolves (explicit path → `~/.vibearound/agents.json` → PATH scan, cached after first discovery).
2. **Project integrations.** If the local daemon is running, project-scoped MCP config and skill files for the agent are installed into the workspace (honoring `integrations.mcp_auto_install` and `integrations.skill_auto_install`). If the daemon is **not** running, VibeAround-managed project integrations are removed instead — stale config must not point the agent at a dead MCP server.
3. **Terminal spawn.** The agent starts in your terminal with the rendered environment. For bridged profiles, its model traffic flows through `127.0.0.1:12358` — so keep the daemon running for the session's lifetime.

## Sessions from launches

Launched CLIs create their own native sessions, which VibeAround discovers:

```bash
va launch sessions                     # resumable sessions across agents/workspaces
va launch archive --agent claude <id>  # hide one from pickers
va launch unarchive --agent claude <id>
```

These sessions appear in the desktop/dashboard resume pickers and can be attached to a chat with `/session --switch <id>` or handed over with the in-CLI handover tool + `/pickup <code>`. That is the bridge between "working in a terminal" and "continuing from a phone" — see [Session lifecycle](../architecture/session-lifecycle.md).

## Launch vs hosted: which to use

| | Launch (your terminal) | Hosted (IM / web chat) |
|---|---|---|
| UI | The agent's own full TUI | Chat bubbles + permission cards |
| Process owner | Your terminal | The daemon (idle-shutdown managed) |
| Model routing | Profile-rendered config | Same profiles, same bridge |
| Continuity | Sessions discoverable, handover via code | Sessions tracked on the thread automatically |
| Survives daemon stop | CLI keeps running; bridged model calls fail until daemon returns | No (agent is daemon-hosted) |

## Troubleshooting

| Symptom | Fix |
|---|---|
| "executable not found" | Install the agent CLI, or set an explicit `executablePath` in the launch profile; delete a stale entry in `~/.vibearound/agents.json` to force a re-scan |
| Terminal opens then closes instantly | Run with `--dry-run` to see the plan; try the command it prints manually to surface the real error |
| Agent starts but model calls fail | Daemon not running (bridged profile), or profile key invalid — check `va status` and the profile |
| Linux: nothing opens | No supported terminal found; install one of the common terminals or set the terminal preference |

---

*Source anchors: `src/launcher/` (va-launch: validation, integrations, spawn), `~/.vibearound/launch/profiles/` schema (launch profile JSON v1), `src/core/src/agent/launch.rs` + `src/core/src/profiles/bridge_launch.rs` (profile rendering), `src/core/src/launch_sessions/` (session discovery), `src/cli/src/args.rs` (va launch commands).*
*Last verified: v0.7.11*
