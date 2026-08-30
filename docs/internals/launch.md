# Launch subsystem

Everything about starting an agent process, in one place: the four launch paths, how environment variables are assembled and injected on each, per-OS terminal handling, argument sources, and where desktop and CLI producers differ. The step-by-step walkthrough of one native launch is [flows/native-launch](flows/native-launch.md); the user guide is [guides/agent-launch](../guides/agent-launch.md).

## The four launch paths

| Path | Trigger | Process owner | Env injection mechanism | Dies with daemon |
|---|---|---|---|---|
| **Native launch** | desktop Launch / `va launch` | your terminal app | generated shell script (`export` / `$env:`) | no |
| **Hosted ACP spawn** | IM / web chat prompt | daemon (supervisor) | process env on `Command` spawn | yes |
| **PTY session** | web terminal / `va session create` | daemon (PTY registry) | process env on pty child | yes |
| **Desktop-app target** | Launch with `claude-desktop` / `codex-desktop` | the vendor GUI app | `open --env` (macOS) / `Start-Process` (Windows) | no |

All four converge on the same profile rendering code — a Kimi profile produces the same `ANTHROPIC_BASE_URL` whether the agent is hosted, launched, or opened as a GUI app.

## Producers: desktop vs CLI

Both producers end at the same boundary: **exec the sibling `va-launch` binary with a launch-profile JSON**. Neither calls the launcher library in-process. Two "profile" concepts meet at this boundary — a **provider profile** (credentials + routing) is rendered *into* a **launch profile** (a native launch request, [schema v1](../reference/configuration.md#launch-profile-json-schema-v1)); `va-launch` only ever sees the latter and never reads provider storage (`profileId` in the JSON is metadata).

| | Desktop | `va` CLI |
|---|---|---|
| Provider prep | Renders the profile itself (`desktop/src/profiles/launcher/`: bridge overlay, codex/claude desktop variants, resume plan), writes a **materialized temp launch profile JSON**, execs `va-launch --profile-path <temp>` | None — `va launch --profile <name>` reads a **saved** profile from `~/.vibearound/launch/profiles/<name>.json`, or `--profile-path` any file; forwards only `--profile`, `--profile-path`, `--dry-run`, `--json` |
| va-launch binary | Tauri sidecar, bundled into the app's executable dir | Shipped in the same npm package directory as `va` |
| Resume | Launch screen picks a session; the resume plan renders the agent's `resume_template` (`cd {cwd} && claude --resume {session_id}`) into the command | Saved profile carries whatever command was materialized into it |
| Dev override | `VIBEAROUND_VA_LAUNCH_BIN` points either producer at a non-packaged launcher (dev/test only) | same |

Consequence: a **saved** CLI launch profile is static — it holds the env that was rendered when it was created (including bridge URLs bound to a `launch_id`). The desktop re-renders per launch.

## Environment assembly, layer by layer

Env is built upstream of `va-launch` and carried in the plan's `env` map (BTreeMap, deduplicated, keys validated against `[A-Za-z_][A-Za-z0-9_]*`).

| Layer | Contents | Applies to |
|---|---|---|
| 1. Base process env | Enriched login-shell env (`process/env.rs`, cached once) so PATH matches the user's shell | hosted, PTY (native launches inherit the terminal's own login env instead) |
| 2. Identity env | Hosted: `VIBEAROUND_CHANNEL_KIND`, `VIBEAROUND_CHAT_ID`, `VIBEAROUND_AGENT_KIND`, `VIBEAROUND_THREAD_ID`, `VIBEAROUND_WORKSPACE_ID`. Launch-materialized: `VIBEAROUND_LAUNCH_ID` (fresh UUID per render), `VIBEAROUND_PROFILE_ID` (normalized; `direct` for none/default/off), `VIBEAROUND_LAUNCH_TARGET` | all profile-driven paths |
| 3. Profile credentials + bridge | Per-agent variable families rendered by `bridge_launch.rs`: Claude → `ANTHROPIC_API_KEY`/`ANTHROPIC_AUTH_TOKEN`/`ANTHROPIC_BASE_URL`/`ANTHROPIC_MODEL` (+custom-model-option and gateway-discovery flags); Codex → `OPENAI_API_KEY` + `-c model_providers.…` **args** rather than env; Gemini → `GEMINI_API_KEY`, `GOOGLE_GEMINI_BASE_URL`, `GEMINI_MODEL`, `GEMINI_DEFAULT_AUTH_TYPE`. For bridged routes the "API key" is a scoped placeholder and the base URL points at `127.0.0.1:12358/va/local-api/…` — real provider keys never enter the env | bridged profiles |
| 4. Config pointer | When the profile renders settings files, they are written under `~/.vibearound/profile-state/<profile-id>/…` and a `ConfigEnvTarget` env var points there — `Directory(env)` or `File { env, rel_path }`. **Deliberately never `CODEX_HOME` / `CLAUDE_CONFIG_DIR`**: overriding agent home dirs would cut the CLI off from the user's own sessions, plugins, and skills | profiles with settings files |
| 5. Proxy | `append_settings_proxy_env`: settings.json proxy exported for direct-to-provider routes only (bridged routes go through the daemon, which applies proxy itself) | direct provider routes |
| 6. Terminal hygiene | Script-level: `unset NO_COLOR`, `TERM=xterm-256color` fallback, `COLORTERM`/`CLICOLOR` defaults; macOS adds terminal-update-suppression vars. PTY equivalent: `NO_COLOR` removed, `PTY_ENV` defaults + theme | native launches, PTY |

## Injection mechanics per path

- **Hosted / PTY:** plain process env on the spawned child (`Command::env` / pty builder) — nothing touches disk.
- **Native launch (macOS/Linux):** `va-launch` writes a one-shot script to `$TMPDIR/vibearound/launch/scripts/script-<uuid>.{command,sh}` (mode 0700). The script **deletes itself first** (`rm -- "$0"`), then `export`s each env pair (unix-shell-escaped), `cd`s to the workspace, and `exec`s the command. Env values therefore transit the filesystem briefly — the self-delete plus 0700 is the hygiene for that.
- **Native launch (Windows):** same idea in PowerShell — self-deleting script with `$env:KEY = '…'` lines (single-quote escaped), window title, color env, `Set-Location`, then the command block.
- **macOS GUI apps:** a shell can't export into a `.app`; the script rewrites `open …` into `open --env KEY=VALUE …` per pair, and an osascript probe template checks whether the app is already running (`macos_app_probe`).
- **`cleanup_paths`:** plan-listed temp files (e.g. desktop-rendered overlay configs) are `rm -f`'d by the script after the command exits, instead of `exec`ing — the script stays alive to clean up.

## Per-OS terminal handling

| OS | Mechanism | Terminal choices |
|---|---|---|
| macOS | `open -b <bundle-id> <script.command>` (bundle id, so the app is found wherever it is installed) | `terminal` (Terminal.app, default), `iterm2` (detected in `/Applications` and `~/Applications`, `iTerm.app` or `iTerm 2.app`); anything else errors |
| Windows | `open::with(script, "powershell.exe")`; app targets use `Start-Process` with `windowsExecutablePath` normalization; `windows_process_probe` checks running processes | `powershell` (default) |
| Linux | spawn script via candidate list | `system-terminal` (default = try in order: `xdg-terminal-exec`, `x-terminal-emulator`, `gnome-terminal --`, `konsole -e`, …) or explicit `gnome-terminal`, `konsole`, `xfce4-terminal`, `xterm`, `kitty`, `alacritty`, `wezterm` |

Preference resolution: explicit choice in the launch profile → persisted terminal config (`terminal_config.rs`, initialized on first use) → platform default. An unsupported combination (e.g. `konsole` on macOS) fails validation rather than falling back silently.

## Argument handling

Three sources merge into the final command line:

1. **Profile command args** (`RenderedProfile::command_args`) — provider routing that must be args, not env: Codex gets `-c model_providers.<id>.base_url=…` overrides; other agents mostly use env.
2. **Per-agent saved args** (`~/.vibearound/agents.json` prefs) — two separate lists per agent: `launch_args.terminal` (native launches) and `launch_args.acp` (hosted spawns). They are intentionally distinct: `--dangerously-skip-permissions` might make sense in your own terminal but not for an IM-driven host.
3. **Resume rendering** — the agent registry's `resume_template` (`cd {cwd} && claude --resume {session_id}`) becomes the command for resume launches; agents without a template (kiro) can't resume natively.

The launcher splits the `command` string into words with quote-aware parsing (single/double quotes, `\"` escapes) and appends `args`, so a command like `claude code --permission-mode acceptEdits` from the registry survives intact.

## Executable resolution (native launches)

1. `executablePath` in the launch profile — validated, used as-is (`windowsExecutablePath` variant for Windows app launches).
2. `agents.<agent>.executable.path` in `~/.vibearound/agents.json`.
3. PATH scan for the command's program; the discovery is **written back** to `agents.json` and trusted thereafter — a stale entry fails validation instead of re-scanning (delete the entry to force rediscovery).

App-launch wrappers (`open -a …`, `Start-Process …`) are treated as native app commands and never cached as CLI executables.

---

*Source anchors: `src/launcher/src/` (platform.rs — scripts and per-OS spawn, plan.rs — ExecutionPlan, executable.rs, terminal_config.rs, lib.rs — TerminalChoice), `src/core/src/agent/launch.rs` (materialize_profile_for_agent, profile-id env), `src/core/src/profiles/{render.rs,runtime.rs,bridge_launch.rs}` (env families, ConfigEnvTarget, profile-state dir), `src/core/src/agent_state.rs` (launch_args.terminal/acp), `src/desktop/src/profiles/launcher/` (desktop producer, resume plan), `src/core/src/process/env.rs` + `src/core/src/pty/runtime.rs` (hosted/PTY env).*
*Last verified: v0.7.11*

<sub>[◀ Module: server](modules/server.md) · [Documentation index](../README.md)</sub>
