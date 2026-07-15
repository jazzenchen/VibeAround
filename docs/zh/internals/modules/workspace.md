# Module: workspace

`src/core/src/workspace/`：对话状态模型，包括 workspaces、threads、route attachments 和 handover codes。如果 [channels](channels.md) 是邮局，本模块就是决定每封信属于哪个对话的档案系统。

## 职责

拥有所有持久化对话状态及其 runtime counterpart。三份 event-sourced stores（workspaces、threads、route attachments）加一张 live `ThreadRuntime` 内存表。[核心概念](../../architecture/concepts.md)页描述的所有东西都在这里实现。

## 关键类型

| Type | File | Role |
|---|---|---|
| `WorkspaceThreadManager` | `manager.rs`, `manager_routes.rs` | 编排器：route→thread resolution、thread creation/close、attachments、external session binding、warm Thread 池 reconcile |
| `ThreadRuntime` | `threads/runtime.rs` | 一个 live thread：durable session id、live host generation、activity/busy/failed state、subagents、prompt serialization |
| `AcpSessionRunner` | `threads/runtime.rs` | 一个 ACP/process generation 的 live `Agent` + callback handler |
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
3. **Thread 的 agent spawn 是 single-flight**（`spawn_lock`），停止的 `AcpSessionRunner` 作为整体替换；同一 thread 的 prompts 由 `prompt_lock` 串行，`cancel` 刻意绕过它。
4. **Closed 对 thread id 是终态**；reopen 意味着新 thread。
5. **Session id 是观测，不是所有权**。Agent 自己的 storage 才是权威；不要伪造 session id。
6. **Warm 回收由压力触发且保持保守**：只有真正的新 Host 启动并超过软上限后才 reconcile；最多回收一个达到闲置门槛、不在忙、不是受保护 Thread、且没有任何常驻子 Agent 的最近最少活动 runtime。没有候选者就允许 overflow。回收保留 runtime/session 与预览记录。

## 已知技术债

- Active route 先命中 runtime/attachment cache，但 thread/workspace projection 仍全量 replay JSONL，需要 benchmark 后再决定 snapshot。
- `ThreadRuntime` 仍有多把独立 mutex 和手工维护的 `busy`/`failed` shadows，应渐进收拢所有权。
- `RouteKey::as_key()/from_key()` 有意 lossy，无法表达 actor/topic；runtime control 已改用 workspace thread id，在定义 versioned persistent route key 前，该形式只能用于 legacy/display。

---

*Source anchors: `src/core/src/workspace/` (manager, threads/, handover, registry, store).*
*Last verified: `codex/im-acp-route-refactor` at `121381f4`（2026-07-11）。*

<sub>[◀ Module: channels](channels.md) · [文档索引](../../README.md) · [Module: process ▶](process.md)</sub>
