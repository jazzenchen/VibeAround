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
| `orphan_sweep` | `orphan.rs` | Startup sweep that kills children left over from a previous daemon crash |
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
4. **Two-layer cleanup**: normal stop cancels, tree-reaps and joins/aborts the bridge before returning; `Supervisor::kill_all_blocking` (a pid-only emergency roster, group-killed synchronously) remains the abrupt-runtime safety net.
5. **Enriched env everywhere**: children spawn through `process::env::command` so PATH matches the user's shell; bypassing it produces "works in terminal, fails in app" bugs.
6. Heartbeat watchdog applies to plugins only; agents crash loudly by design (`Never`) so the owning thread decides.
7. A watchdog restart uses the same bounded stop path as a manual restart: cancel, tree-reap, wait, then abort a stubborn bridge before the replacement generation can publish.
8. Repeated `OnCrash` failures back off exponentially from the configured delay to five minutes. A heartbeat or manual start/restart resets the failure budget.
9. Stop is higher priority than restart: if another lifecycle operation owns cleanup, Stop/shutdown waits, re-acquires the barrier, publishes `Stopped`, and reaps any replacement that raced publication. Waiting alone is never treated as a successful stop.

## Known debt

- Unix descendants are terminated through a process group; Windows still needs a Job Object rather than relying on `taskkill`.
- `Supervisor::global()` still impedes test isolation and scoped dependency ownership even though each active generation now has one owner; the next boundary is injected, scoped supervisor handles.
- Restart has bounded exponential backoff, but still lacks jitter, a failure-window budget and an explicit circuit-breaker/manual-reset state.
- `Running` currently means the child and bridge generation were published; channel protocol/platform readiness still needs a separate handshaking/ready signal.

---

*Source anchors: `src/core/src/process/` (supervisor, supervisor/model, supervisor/generation, bridge, registry, acp_transport, env, kill, log).*
*Last verified: `codex/im-acp-route-refactor` at `de10c0e0` (2026-07-11).*

<sub>[◀ Module: workspace](workspace.md) · [Documentation index](../../README.md) · [Module: agent ▶](agent.md)</sub>
