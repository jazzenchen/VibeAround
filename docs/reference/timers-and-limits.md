# Timers and limits

Every timeout, TTL, interval, and size limit in one table. **This page is the single authority for these numbers** — other pages link here instead of restating them; if a value changes in code, change it here and nowhere else in the docs.

## Lifecycles and TTLs

| Value | What it governs | Defined in |
|---|---|---|
| 10 minutes | Host agent idle shutdown — agent process stops, thread stays open, next prompt resumes | `src/core/src/workspace/manager.rs` (`AGENT_HOST_IDLE_SHUTDOWN_DELAY`) |
| 120 seconds | Handover pickup code TTL (4-character code, one-shot) | `src/core/src/workspace/handoff.rs` |
| 60 seconds | Browser pairing code TTL (6-digit code, refreshable) | `src/core/src/auth/pair.rs` (`CODE_TTL`) |
| 600 seconds | Preview **share** link lifetime (owner links never expire) | `src/core/src/previews/store.rs` (`SHARE_TTL_SECS`) |
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
| 64 MB | Max request body on local bridge endpoints (large context payloads) | `src/server/src/web_server/mod.rs` (`LOCAL_BRIDGE_BODY_LIMIT_BYTES`) |
| 64 | Channel input shard workers — same route strictly ordered, routes parallel | `src/server/src/lib.rs` (`CHANNEL_INPUT_WORKER_COUNT`) |
| 4 chars / 32-char alphabet | Handover code format | `src/core/src/workspace/handoff.rs` |
| 6 digits | Pairing code format | `src/core/src/auth/pair.rs` |

## Network defaults

| Value | What it governs | Defined in |
|---|---|---|
| `12358` | Daemon port (HTTP, WS, MCP, bridge) | `src/core/src/config.rs` (`DEFAULT_PORT`) |
| `127.0.0.1` | Bind address; bridge endpoints additionally reject non-loopback callers | `src/server/src/` |
| 3 seconds | Web listener graceful-shutdown timeout before abort | `src/server/src/lib.rs` (`WEB_SHUTDOWN_TIMEOUT`) |

No timeout exists on permission requests by design — an agent turn waits for the human as long as it takes; termination is guaranteed by cancellation paths instead ([permission flow](../internals/flows/permission.md)).

---

*Source anchors: the "Defined in" column above — each row names its constant.*
*Last verified: v0.7.11*

<sub>[◀ API surfaces reference](api-surfaces.md) · [Documentation index](../README.md) · [Provider endpoints reference ▶](provider-endpoints.md)</sub>
