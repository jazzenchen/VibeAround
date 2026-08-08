# Timers and limits

Every timeout, TTL, interval, and size limit in one table. **This page is the single authority for these numbers** — other pages link here instead of restating them; if a value changes in code, change it here and nowhere else in the docs.

## Lifecycles and TTLs

| Value | What it governs | Defined in |
|---|---|---|
| 10 minutes | Minimum idle age before a resident warm thread is eligible for pressure eviction; this is not a timer | `src/core/src/workspace/manager.rs` (`WARM_THREAD_MIN_IDLE`) |
| 120 seconds | Handover pickup code TTL (4-character code, one-shot) | `src/core/src/workspace/handover.rs` |
| 60 seconds | Browser pairing code TTL (6-digit code, refreshable) | `src/core/src/auth/pair.rs` (`CODE_TTL`) |
| 600 seconds | Markdown preview **share transaction** lifetime: URL ID, reusable access code, and browser grant expire together (owner access follows the preview session; Server previews do not mint shares) | `src/core/src/previews/store.rs` (`SHARE_TTL_SECS`) |
| per daemon start | Dashboard auth token rotation — every restart invalidates all previous URLs | `src/core/src/auth/token.rs` |

## Supervision

| Value | What it governs | Defined in |
|---|---|---|
| 15 seconds | Channel plugin heartbeat cadence (`_va/heartbeat`) | plugin SDK contract |
| 90 seconds | Plugin watchdog window — no heartbeat for this long → kill + respawn | `RestartPolicy::OnCrash { watchdog }` |
| 5 seconds | Supervisor tick — granularity of crash-restart scheduling | `src/core/src/process/supervisor.rs` (`TICK_INTERVAL`) |
| never | Agent processes are not auto-respawned (`RestartPolicy::Never`) — crashes surface to the owning thread | `src/core/src/agent/runtime.rs` |

## Sizes and counts

| Value | What it governs | Defined in |
|---|---|---|
| 4 (soft limit) | Resident warm threads; after a genuinely new host starts above the limit, evict at most one eligible least-recently-active thread, or allow overflow if none qualifies | `src/core/src/workspace/manager.rs` (`MAX_WARM_THREADS`) |
| 64 MB | Max request body on local bridge endpoints (large context payloads) | `src/server/src/web_server/mod.rs` (`LOCAL_BRIDGE_BODY_LIMIT_BYTES`) |
| 64 | Channel input shard workers — same route strictly ordered, routes parallel | `src/server/src/lib.rs` (`CHANNEL_INPUT_WORKER_COUNT`) |
| 4 chars / 32-char alphabet | Handover code format | `src/core/src/workspace/handover.rs` |
| 6 digits | Pairing code format | `src/core/src/auth/pair.rs` |

## Network defaults

| Value | What it governs | Defined in |
|---|---|---|
| `12358` | Daemon port (HTTP, WS, MCP, bridge) | `src/core/src/config.rs` (`DEFAULT_PORT`) |
| `127.0.0.1` | Bind address; bridge endpoints additionally reject non-loopback callers | `src/server/src/` |
| 3 seconds | Web listener graceful-shutdown timeout before abort | `src/server/src/lib.rs` (`WEB_SHUTDOWN_TIMEOUT`) |

There is no fixed idle-shutdown deadline for hosted agents or Web Chat. A warm host is considered for eviction only when a genuinely new host has started above the soft pool limit. The candidate must be idle for at least the threshold above, not busy, not the newly started thread, and have no resident subagents. If no candidate qualifies, the pool is allowed to overflow.

No timeout exists on permission requests by design — an agent turn waits for the human as long as it takes; termination is guaranteed by cancellation paths instead ([permission flow](../internals/flows/permission.md)).

---

*Source anchors: the "Defined in" column above — each row names its constant.*
*Last verified: v0.7.11*

<sub>[◀ API surfaces reference](api-surfaces.md) · [Documentation index](../README.md) · [Provider endpoints reference ▶](provider-endpoints.md)</sub>
