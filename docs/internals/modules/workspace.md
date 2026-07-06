# Module: workspace

`src/core/src/workspace/` — the conversation state model: workspaces, threads, route attachments, and handover codes. If [channels](channels.md) is the postal service, this module is the filing system deciding which conversation every letter belongs to.

## Responsibility

Own all persistent conversation state and its runtime counterparts. Three event-sourced stores (workspaces, threads, route attachments) plus an in-memory map of live `ThreadRuntime`s. Everything the [Concepts](../../architecture/concepts.md) page describes is implemented here.

## Key types

| Type | File | Role |
|---|---|---|
| `WorkspaceThreadManager` | `manager.rs` | The orchestrator: route→thread resolution, thread creation/close, attachments, external session binding, idle shutdown |
| `ThreadRuntime` | `threads/runtime.rs` | One live thread: host agent handle, session id, busy/failed state, subagents, prompt serialization |
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
3. **A thread's agent spawn is single-flight** (`spawn_lock` in `ThreadRuntime`) and prompts on one thread are serialized (`prompt_lock`); `cancel` intentionally bypasses the prompt lock — preserve that or cancellation dies.
4. **Closed is terminal** for a thread id; reopening means a new thread.
5. **Session ids are observations, not ownership** — the agent's own storage is authoritative; never fabricate one.

## Known debt

- Projections re-read and replay whole JSONL files on the hot path; thread/workspace stores never compact (attachments do). Remediation H1 — the top-priority fix.
- `ThreadRuntime` is 10 independent mutexes with manually maintained `busy`/`failed` shadows; planned actor-model rewrite (remediation H2, with the supervisor-tree milestone).
- `route_locks` map never prunes entries (remediation L8).
- `RouteKey::as_key()` drops `bot_id` — latent for multi-bot futures (remediation L9).

---

*Source anchors: `src/core/src/workspace/` (manager, threads/, handover, registry, store), `reports/architecture-review-remediation-2026-07-04.md` (H1, H2, L8, L9).*
*Last verified: v0.7.11*

<sub>[◀ Module: channels](channels.md) · [Documentation index](../../README.md) · [Module: process ▶](process.md)</sub>
