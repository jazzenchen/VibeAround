# Flow: agent launch

From "Launch" click (or `va launch`) to an agent CLI running in your terminal. The architecture rule shaping this flow: provider/runtime preparation happens **before** the native launcher; `va-launch` itself is a standalone binary that must work without the desktop app. Mechanism details — env layers, per-OS scripts, argument sources — live in the [launch subsystem deep-dive](../launch.md).

## Hop by hop

```text
UI / CLI ──1─► provider prep ──2─► launch profile JSON ──3─► va-launch
                                                               │4 validate
                                                               │5 workspace preparation
                                                               ▼6
                                                        terminal spawn
                                                               │7
                                                        CLI ──► /va/local-api (models)
```

**1. Selection.** Desktop Launch screen or `va launch --profile <name>` picks agent + workspace + model profile + terminal preference.

**2. Provider prep (upstream of the launcher).** For bridged profiles, the profile is rendered into concrete launch material: env vars, per-agent config overlays (Codex `model_providers` args, Claude settings, Gemini/opencode/pi variants), and local-api base URLs scoped to this launch. The `direct` profile skips all of this. Desktop-app targets get their overlay variant instead.
→ `src/core/src/agent/launch.rs`, `src/core/src/profiles/bridge_launch.rs`, `render.rs`

**3. Launch profile JSON.** Everything is serialized into a schema-v1 launch profile: agent, workspace, terminal, command/executable override, env, args, window label. Saved profiles live in `~/.vibearound/launch/profiles/`; the desktop writes a materialized temp one. Unknown fields are rejected — a provider profile handed to the launcher by mistake fails loudly.
→ launch profile schema (internal notes), `src/launcher/`

**4. va-launch validates.** The CLI/desktop **execs the sibling `va-launch` binary** (no in-process launcher). It validates the workspace, resolves the agent executable (explicit path → `agents.json` → PATH scan, cached), and validates the terminal choice. `--dry-run` stops here and prints the plan.
→ `src/launcher/` (resolution order), `~/.vibearound/agents.json`

**5. Workspace preparation.** va-launch replaces the agent's VibeAround-reserved project skills. When `auth-mcp.json` is available, it also writes the current daemon MCP credential into project config. Desktop-app targets prepare their companion CLI: `claude-desktop` → `claude`, `codex-desktop` → `codex`.
→ `src/launcher/`, `src/core/src/agent/{mcp,skills}.rs`

**6. Terminal spawn.** The agent opens in the chosen terminal (Terminal.app/iTerm2, PowerShell, or a detected Linux terminal); desktop-app targets go through `open -a` / `Start-Process` instead. va-launch's job ends when the terminal is spawned — the CLI process belongs to you, not the daemon.

**7. Runtime relationship.** A bridged CLI now calls `127.0.0.1:12358` for models ([bridge request flow](bridge-request.md)) and can use the injected MCP tools (handover, previews, subagents). Its native sessions are discovered by the daemon and become resumable from every other surface ([handover flow](handover.md)).

## Failure behavior

| Failure | Result |
|---|---|
| Executable not found | Validation error before anything spawns; clear the stale `agents.json` entry to force a re-scan |
| Workspace missing | Validation error |
| Daemon down at launch | Skills sync; MCP config is unchanged when no credential is available; bridged model calls fail |
| Terminal not found (Linux) | Launch error listing what was tried |
| Wrong JSON shape | Schema rejection (unknown fields) — the producer's bug surfaces immediately |

---

*Source anchors: `src/launcher/`, `src/core/src/agent/launch.rs`, `src/core/src/profiles/bridge_launch.rs`, `src/core/src/agent/{mcp,skills}.rs`, `src/cli/src/args.rs`.*

<sub>[◀ Flow: bridge request](bridge-request.md) · [Documentation index](../../README.md) · [Flow: handover ▶](handover.md)</sub>
