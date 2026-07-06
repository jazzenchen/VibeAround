# Module: agent

`src/core/src/agent/` — one live ACP connection to a coding CLI, and everything needed to start it correctly: launch rendering, config injection, install helpers.

## Responsibility

Wrap a single agent subprocess behind a typed handle (`Agent`) speaking ACP, and prepare the ground it runs on: profile-derived env/args, VibeAround's MCP + skills written into the agent's global config, startup session semantics (fresh / load / resume).

## Key types

| Type | File | Role |
|---|---|---|
| `Agent` | `runtime.rs` | The handle: `prompt`, `cancel`, `shutdown`, spawn via the supervisor (policy `Never`); stdio ACP only — no transport trait until a second transport exists |
| `AgentClientHandler` | `runtime.rs` | Southbound callback trait: `session_notification`, `request_permission`, `prompt_finished` — implemented by `channels::bridge_handler` and the subagent handler |
| `AcpAgentBridge` | `bridge.rs` | ProcessBridge impl: drives the ACP connection, handles startup session attach/fallback |
| `StartupSession` | `runtime.rs` | Fresh vs resume-by-id startup semantics |
| `launch` | `launch.rs` | Profile materialization for hosted + native launches (`DIRECT_PROFILE_ID`, credential env, profile-id env) |
| `mcp` / `skills` | `mcp.rs`, `skills.rs` | Global + project-scoped config injection per agent (paths from the registry's `global_config`) |
| `install` | `install.rs` | Agent CLI / ACP adapter installation (npm packages from the registry) |

## Interactions

- **← workspace:** `ThreadRuntime::ensure_agent` is the main caller; subagent spawning uses the same `Agent::spawn`.
- **→ process:** spawn/shutdown delegate to the `Supervisor`.
- **→ profiles:** `launch.rs` pulls rendered credentials and bridge URLs.
- **→ resources:** all agent identity (ids, aliases, adapter packages, config paths) comes from the embedded registry — never hardcode an agent id in logic.

## Invariants — do not break

1. **Crashes surface, not auto-heal**: restart policy is `Never`; the owning thread decides whether to respawn. Do not add silent retry here.
2. **Startup-session fallback clears the stale id**: if resume fails and the bridge fell back to a fresh agent, the recorded candidate session id must be cleared so a real one is created — otherwise prompts target a dead session.
3. **Config injection is idempotent and reversible**: MCP/skill writes are marked as VibeAround-managed so launch-time cleanup can remove them when the daemon is down.
4. **Registry-driven identity**: adding an agent is an `agents.json` change (adapter package, pty command, config paths), not new match arms — keep it that way where possible.

## Known debt

- `profiles/bridge_launch.rs` (used from `launch.rs`) hardcodes launch-target match arms (`"claude" | "codex" | …`) — acceptable today, listed for catalog-driven cleanup with M7's URL-shape consolidation.

---

*Source anchors: `src/core/src/agent/` (runtime, bridge, launch, mcp, skills, install), `src/resources/agents.json`, `src/core/src/resources.rs`.*
*Last verified: v0.7.11*

<sub>[◀ Module: process](process.md) · [Documentation index](../../README.md) · [Module: profiles ▶](profiles.md)</sub>
