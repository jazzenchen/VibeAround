# Module: agent

`src/core/src/agent/` — one live ACP connection to a coding CLI, launch preparation, and install helpers.

## Responsibility

Wrap a single agent subprocess behind a typed ACP handle and prepare profile env/args, project skills, MCP config, and startup session state.

## Key types

| Type | File | Role |
|---|---|---|
| `Agent` | `runtime.rs` | One live ACP/process generation: `prompt`, `cancel`, `shutdown`, spawn via supervisor policy `Never` |
| `AgentClientHandler` | `runtime.rs` | Southbound callback trait: `session_notification`, `request_permission`, `prompt_finished` — implemented by `channels::bridge_handler` and the subagent handler |
| `AcpAgentBridge` | `bridge.rs` | ProcessBridge impl: drives the ACP connection, handles startup session attach/fallback |
| `StartupSession` | `runtime.rs` | Fresh vs resume-by-id startup semantics |
| `launch` | `launch.rs` | Profile materialization for hosted + native launches (`DIRECT_PROFILE_ID`, credential env, profile-id env) |
| `mcp` / `skills` | `mcp.rs`, `skills.rs` | Project MCP config and reserved skill synchronization |
| `install` | `install.rs` | Agent CLI / ACP adapter installation (npm packages from the registry) |

## Interactions

- **← workspace:** `ThreadRuntime::ensure_agent` is the main caller; subagent spawning uses the same `Agent::spawn`.
- **→ process:** spawn/shutdown delegate to the `Supervisor`.
- **→ profiles:** `launch.rs` pulls rendered credentials and bridge URLs.
- **→ resources:** all agent identity (ids, aliases, adapter packages, config paths) comes from the embedded registry — never hardcode an agent id in logic.

## Invariants — do not break

1. **Crashes surface, not auto-heal**: restart policy is `Never`; the owning thread decides whether to respawn. Do not add silent retry here.
2. **Startup-session fallback clears the stale id**: if resume fails and the bridge fell back to a fresh agent, the recorded candidate session id must be cleared so a real one is created — otherwise prompts target a dead session.
3. **Launch preparation is deterministic**: reserved skills are replaced on every launch; MCP config uses the current `auth-mcp.json` credential.
4. **Registry-driven identity**: adding an agent is an `agents.json` change (adapter package, pty command, config paths), not new match arms — keep it that way where possible.
5. `Agent::shutdown` must not return until the supervisor has reaped the child and joined or bounded-aborted that generation's bridge task.

## Known debt

- `profiles/bridge_launch.rs` (used from `launch.rs`) hardcodes launch-target match arms (`"claude" | "codex" | …`) — acceptable today, listed for catalog-driven cleanup with M7's URL-shape consolidation.

---

*Source anchors: `src/core/src/agent/` (runtime, bridge, launch, mcp, skills, install), `src/resources/agents.json`, `src/core/src/resources.rs`.*
*Last verified: `codex/im-acp-route-refactor` at `924d4c60` (2026-07-11).*

<sub>[◀ Module: process](process.md) · [Documentation index](../../README.md) · [Module: profiles ▶](profiles.md)</sub>
