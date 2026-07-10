# Module: workspace

`src/core/src/workspace/` — the conversation state model: workspaces, threads, route attachments, and handover codes. If [channels](channels.md) is the postal service, this module is the filing system deciding which conversation every letter belongs to.

## Responsibility

Own all persistent conversation state and its runtime counterparts. Three event-sourced stores (workspaces, threads, route attachments) plus an in-memory map of live `ThreadRuntime`s. Everything the [Concepts](../../architecture/concepts.md) page describes is implemented here.

## Key types

| Type | File | Role |
|---|---|---|
| `WorkspaceThreadManager` | `manager.rs` | The orchestrator: route→thread resolution, thread creation/close, attachments, external session binding, idle shutdown |
| `ThreadRuntime` | `threads/runtime.rs` | One live thread: durable session id, live host generation, busy/failed state, subagents, prompt serialization |
| `AcpSessionRunner` | `threads/runtime.rs` | Live host `Agent` + callback handler for exactly one ACP/process generation |
| `WorkspaceEventStore` / `ThreadEventStore` / `RouteAttachmentEventStore` | `store.rs`, `threads/store.rs`, `threads/attachment.rs` | Append-only JSONL logs + projection replay |
| `WorkspaceThread` / `ThreadProjection` | `threads/store.rs` | Persistent thread record: status, host binding, agent sessions, multi-agent turns |
| `HostBinding` | `threads/store.rs` | (agent id, profile id) pair hosting a thread |
| `handover` | `handover.rs` | 4-char / 120 s one-shot pickup codes |

## Interactions

- **← channels:** every prompt/command lands on `WorkspaceThreadManager`.
- **→ agent:** `ThreadRuntime::ensure_agent` spawns `Agent` (which registers with `process::Supervisor`).
- **→ profiles:** host binding's profile is materialized at agent spawn.
- **→ launch_sessions:** external session resolution during handover/resume.
- **→ state:** implements `StateSource` so dashboards poll `runtime_entries` + subscribe to changes.

## Invariants — do not break

1. **Persist before apply**: every state change is an event appended to its store *before* observable behavior depends on it; a crash replays to the same state.
2. **One open thread per route**: `resolve_route_runtime` holds the per-route lock across check-create-attach; new resolution paths must take the same lock.
3. **A thread's agent spawn is single-flight** (`spawn_lock`), and a stopped `AcpSessionRunner` is replaced as a unit. Prompts on one thread are serialized (`prompt_lock`); cancel intentionally bypasses it.
4. **Closed is terminal** for a thread id; reopening means a new thread.
5. **Session ids are observations, not ownership** — the agent's own storage is authoritative; never fabricate one.

## Known debt

- Active routes hit runtime/attachment caches first, but thread/workspace projections still replay their full JSONL stores and need measurement before snapshotting.
- `ThreadRuntime` still has many independent mutexes with manually maintained `busy`/`failed` shadows; ownership should be consolidated incrementally.
- `RouteKey::as_key()`/`from_key()` are intentionally lossy and cannot represent actor/topic identity; Dashboard runtime control still collides until it gets a versioned `runtime_id`.

---

*Source anchors: `src/core/src/workspace/` (manager, threads/, handover, registry, store).*
*Last verified: `codex/im-acp-route-refactor` at `121381f4` (2026-07-11).*

<sub>[◀ Module: channels](channels.md) · [Documentation index](../../README.md) · [Module: process ▶](process.md)</sub>
