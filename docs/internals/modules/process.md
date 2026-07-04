# Module: process

`src/core/src/process/` — every subprocess the daemon owns goes through here: spawning, supervision, restart policy, watchdogs, and last-resort cleanup. If it forks, this module is accountable for it dying properly.

## Responsibility

Provide one supervised path for child processes (channel plugins, agent ACP adapters) so spawn/restart/cleanup logic exists exactly once. The supervisor knows *nothing* about the protocol spoken over the child's pipes — that is the bridge's job, supplied by the owning module.

## Key types

| Type | File | Role |
|---|---|---|
| `Supervisor` | `supervisor.rs` | Owns process lifecycles: state machine (NotStarted→Spawning→Running→Crashed→…), 5 s tick loop, restart policies, status broadcast |
| `SpawnSpec` | `supervisor.rs` | Program + args + cwd + env recipe, re-used on every respawn |
| `RestartPolicy` | `supervisor.rs` | `Never` (agents, PTY) or `OnCrash { delay, watchdog }` (plugins — 90 s heartbeat watchdog) |
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
3. **Two-layer cleanup**: graceful path (cancel + drop) is the supervisor's; `ChildRegistry::kill_all` is the synchronous safety net when the runtime tears down first. Both must stay.
4. **Enriched env everywhere**: children spawn through `process::env::command` so PATH matches the user's shell; bypassing it produces "works in terminal, fails in app" bugs.
5. Heartbeat watchdog applies to plugins only; agents crash loudly by design (`Never`) so the owning thread decides.

## Known debt

- `Supervisor::global()` and `ChildRegistry::global()` singletons impede test isolation — planned: supervisor-tree refactor absorbs the registry into the supervisor and injects `Arc<Supervisor>` (remediation M5 + the locked supervisor-tree direction; OS descendants to cascade via pgid).

---

*Source anchors: `src/core/src/process/` (supervisor, bridge, registry, acp_transport, env, kill, log), `reports/architecture-review-remediation-2026-07-04.md` (M5, §3).*
*Last verified: v0.7.11*
