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
| `Lease` | `lease.rs` | Kernel-held guarantee that no child outlives the daemon. Unix: a pipe-bound `sh` reaper; the supervisor writes `add <pgid>` right after a spawn succeeds and `del <pgid>` after reaping, and the reaper kills what is left when the pipe closes. Windows: a kill-on-close Job Object; each child is assigned right after spawn and its descendants inherit membership. Deliberately uncovered: the ~1 ms between the spawn and the registration (a daemon killed exactly then leaves that one child — a bug to fix or the user's own doing), and on Unix the reaper itself being killed by hand (later children run uncovered, with a warning) |
| `AcpTransport` wrapper | `acp_transport.rs` | ACP line transport + explicit EOF signal so the supervisor observes child death |
| `env` | `env.rs` | Enriched login-shell environment (cached once) injected into every child |

## Interactions

- **← channels:** `ChannelMonitor` registers plugin manifests; the bridge factory re-points `PluginHost` at the new runtime each respawn.
- **← agent:** `Agent::spawn` registers ACP adapters with policy `Never`.
- **← server:** daemon shutdown calls `shutdown_all` (also the in-process hot-restart path, where the OS-level lease never fires); nothing runs at daemon start.
- **→ nothing above it** — this module is a leaf; it must not know about threads, routes, or profiles.

## Invariants — do not break

1. **Fresh bridge per spawn**: one-shot state lives in the bridge, never in the factory closure; a respawned process must not see its predecessor's state.
2. **The supervisor never interprets pipe content** — protocol concerns stay in bridges.
3. **One active generation record**: registry id, cancel sender and bridge task move together under a generation id. A stale bridge exit may reap only its own registry id and must not mutate a newer generation.
4. **Two-layer cleanup**: an orderly stop cancels, tree-reaps and joins/aborts the bridge before returning; the process lease (`lease.rs`) covers every other way the daemon can die — the kernel closes the lease when the daemon exits and the leased process groups are terminated without any daemon-side code running.
5. **Enriched env everywhere**: children spawn through `process::env::command` so PATH matches the user's shell; bypassing it produces "works in terminal, fails in app" bugs.
6. Heartbeat watchdog applies to plugins only; agents crash loudly by design (`Never`) so the owning thread decides.
7. A watchdog restart uses the same bounded stop path as a manual restart: cancel, tree-reap, wait, then abort a stubborn bridge before the replacement generation can publish.
8. Repeated `OnCrash` failures back off exponentially from the configured delay to five minutes. A heartbeat or manual start/restart resets the failure budget.
9. Stop is higher priority than restart: if another lifecycle operation owns cleanup, Stop/shutdown waits, re-acquires the barrier, publishes `Stopped`, and reaps any replacement that raced publication. Waiting alone is never treated as a successful stop.

## Known debt

- The Windows Job Object path was written without a Windows machine; it awaits a run of `cargo test -p common process::` and a hard kill of the real daemon on Windows.
- Not leased on purpose: installer-style one-shot children (`spawn_tree_killable`: npm installs, startkit scripts) and the desktop onboarding auth script (short-lived, user-attended).
- `Supervisor::global()` still impedes test isolation and scoped dependency ownership even though each active generation now has one owner; the next boundary is injected, scoped supervisor handles.
- Restart has bounded exponential backoff, but still lacks jitter, a failure-window budget and an explicit circuit-breaker/manual-reset state.
- `Running` currently means the child and bridge generation were published; channel protocol/platform readiness still needs a separate handshaking/ready signal.

---

*Source anchors: `src/core/src/process/` (supervisor, supervisor/model, supervisor/generation, bridge, lease, acp_transport, env, kill, log).*
*Last verified: `refactor/supervisor-lease` (2026-09-02).*

<sub>[◀ Module: workspace](workspace.md) · [Documentation index](../../README.md) · [Module: agent ▶](agent.md)</sub>
