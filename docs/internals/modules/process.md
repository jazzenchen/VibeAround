# Module: process

`src/core/src/process/` — every subprocess the daemon owns goes through here: spawning, supervision, restart policy, watchdogs, and last-resort cleanup. If it forks, this module is accountable for it dying properly.

## Responsibility

Provide one supervised path for child processes (channel plugins, agent ACP adapters) so spawn/restart/cleanup logic exists exactly once. The supervisor knows *nothing* about the protocol spoken over the child's pipes — that is the bridge's job, supplied by the owning module.

## Key types

| Type | File | Role |
|---|---|---|
| `Supervisor` | `supervisor.rs` | Public lifecycle API, process table, desired transitions, scoped unregister/shutdown |
| process model | `supervisor/model.rs` | `SpawnSpec`, status/policy, pending child and generation ownership state |
| generation engine | `supervisor/generation.rs` | One spawn/bridge/reap generation, tagged exit handling, process-tree termination |
| `ProcessBridge` / `BridgeFactory` | `bridge.rs` | The protocol driver contract: factory invoked fresh per (re)spawn, handed the stdio pipes |
| `ChildRegistry` | `registry.rs` | Global table of live children; `kill_all()` safety net + startup `orphan_sweep()` |
| `AcpTransport` wrapper | `acp_transport.rs` | ACP line transport + explicit EOF signal so the supervisor observes child death |
| `env` | `env.rs` | Enriched login-shell environment (cached once) injected into every child |

## Interactions

- **← channels:** `ChannelMonitor` registers plugin manifests; the bridge factory re-points `PluginHost` at the new runtime each respawn.
- **← agent:** `Agent::spawn` registers ACP adapters with policy `Never`.
- **← server:** daemon shutdown calls `kill_all`; daemon start calls `orphan_sweep`.
- **→ nothing above it** — this module is a leaf; it must not know about threads, routes, or profiles.

## Invariants — do not break

1. **Fresh bridge per spawn**: one-shot state lives in the bridge, never in the factory closure; a respawned process must not see its predecessor's state.
2. **The supervisor never interprets pipe content** — protocol concerns stay in bridges.
3. **One active generation record**: registry id, cancel sender and bridge task move together under a generation id. A stale bridge exit may reap only its own registry id and must not mutate a newer generation.
4. **Two-layer cleanup**: normal stop cancels, tree-reaps and joins/aborts the bridge before returning; `ChildRegistry::kill_all` remains the abrupt-runtime safety net.
5. **Enriched env everywhere**: children spawn through `process::env::command` so PATH matches the user's shell; bypassing it produces "works in terminal, fails in app" bugs.
6. Heartbeat watchdog applies to plugins only; agents crash loudly by design (`Never`) so the owning thread decides.

## Known debt

- Unix descendants are terminated through a process group; Windows still needs a Job Object rather than relying on `taskkill`.
- `Supervisor::global()` and `ChildRegistry::global()` still impede test isolation even though each active generation now has one logical owner.
- Restart uses a fixed delay; exponential backoff, jitter and a failure budget/circuit breaker remain open.

---

*Source anchors: `src/core/src/process/` (supervisor, supervisor/model, supervisor/generation, bridge, registry, acp_transport, env, kill, log).*
*Last verified: `codex/im-acp-route-refactor` at `924d4c60` (2026-07-11).*

<sub>[◀ Module: workspace](workspace.md) · [Documentation index](../../README.md) · [Module: agent ▶](agent.md)</sub>
