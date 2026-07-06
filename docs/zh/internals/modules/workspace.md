# Module: workspace

`src/core/src/workspace/`：对话状态模型，包括 workspaces、threads、route attachments 和 handover codes。如果 [channels](channels.md) 是邮局，本模块就是决定每封信属于哪个对话的档案系统。

## 职责

拥有所有持久化对话状态及其 runtime counterpart。三份 event-sourced stores（workspaces、threads、route attachments）加一张 live `ThreadRuntime` 内存表。[核心概念](../../architecture/concepts.md)页描述的所有东西都在这里实现。

## 关键类型

| Type | File | Role |
|---|---|---|
| `WorkspaceThreadManager` | `manager.rs` | 编排器：route→thread resolution、thread creation/close、attachments、external session binding、idle shutdown |
| `ThreadRuntime` | `threads/runtime.rs` | 一个 live thread：host agent handle、session id、busy/failed state、subagents、prompt serialization |
| `WorkspaceEventStore` / `ThreadEventStore` / `RouteAttachmentEventStore` | `store.rs`, `threads/store.rs`, `threads/attachment.rs` | Append-only JSONL logs + projection replay |
| `WorkspaceThread` / `ThreadProjection` | `threads/store.rs` | 持久化 thread record：status、host binding、agent sessions、multi-agent turns |
| `HostBinding` | `threads/store.rs` | 托管 thread 的 `(agent id, profile id)` pair |
| `handover` | `handover.rs` | 4 字符 / 120 秒 / 一次性 pickup codes |

## 交互

- **← channels：** 每个 prompt/command 都落到 `WorkspaceThreadManager`。
- **→ agent：** `ThreadRuntime::ensure_agent` spawn `Agent`（后者注册进 `process::Supervisor`）。
- **→ profiles：** agent spawn 时物化 host binding 的 profile。
- **→ launch_sessions：** handover/resume 时解析 external session。
- **→ state：** 实现 `StateSource`，让 dashboards poll `runtime_entries` 并订阅变更。

## 不变量：不要破坏

1. **先持久化，再应用**：每个状态变化都要先 append 到 store，然后才能让可观察行为依赖它；crash 后 replay 到同一状态。
2. **每个 route 一个 open thread**：`resolve_route_runtime` 在 check-create-attach 期间持有 per-route lock；新的 resolution path 也必须拿同一把锁。
3. **Thread 的 agent spawn 是 single-flight**（`ThreadRuntime` 里的 `spawn_lock`），同一 thread 的 prompts 串行化（`prompt_lock`）；`cancel` 刻意绕过 prompt lock，必须保留，否则 cancellation 会死。
4. **Closed 对 thread id 是终态**；reopen 意味着新 thread。
5. **Session id 是观测，不是所有权**。Agent 自己的 storage 才是权威；不要伪造 session id。

## 已知技术债

- Projections 在 hot path 上重读并 replay 整个 JSONL 文件；thread/workspace stores 从不 compact（attachments 会）。remediation H1，最高优先级。
- `ThreadRuntime` 是 10 个独立 mutex，加手工维护的 `busy`/`failed` shadows；计划改成 actor model（remediation H2，随 supervisor-tree 里程碑）。
- `route_locks` map 从不 prune entries（remediation L8）。
- `RouteKey::as_key()` 丢弃 `bot_id`，对未来 multi-bot 是潜在问题（remediation L9）。

---

*Source anchors: `src/core/src/workspace/` (manager, threads/, handover, registry, store), `reports/architecture-review-remediation-2026-07-04.md` (H1, H2, L8, L9).*
*Last verified: v0.7.11*

<sub>[◀ Module: channels](channels.md) · [文档索引](../../README.md) · [Module: process ▶](process.md)</sub>
