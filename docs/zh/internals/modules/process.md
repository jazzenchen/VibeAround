# Module: process

`src/core/src/process/`：daemon 拥有的每个子进程都经过这里，包括 spawn、supervision、restart policy、watchdog 和兜底清理。只要 fork 了，本模块就负责确保它正确死亡。

## 职责

为 child processes（channel plugins、agent ACP adapters）提供唯一的 supervised path，让 spawn/restart/cleanup 逻辑只存在一份。Supervisor 对 child pipes 上传输的协议**一无所知**，协议是 bridge 的责任，由所属模块提供。

## 关键类型

| Type | File | Role |
|---|---|---|
| `Supervisor` | `supervisor.rs` | 公共生命周期 API、process table、desired transitions 与 scoped shutdown |
| process model | `supervisor/model.rs` | `SpawnSpec`、status/policy、pending child 和 generation ownership |
| generation engine | `supervisor/generation.rs` | 单次 spawn/bridge/reap、tagged exit 与 process-tree terminate |
| `ProcessBridge` / `BridgeFactory` | `bridge.rs` | 协议驱动 contract：每次 (re)spawn 都 fresh 调用 factory，并交入 stdio pipes |
| `ChildRegistry` | `registry.rs` | Live children 全局表；`kill_all()` safety net + startup `orphan_sweep()` |
| `AcpTransport` wrapper | `acp_transport.rs` | ACP line transport + 显式 EOF signal，让 supervisor 观察 child death |
| `env` | `env.rs` | 加强过的 login-shell environment（缓存一次），注入每个 child |

## 交互

- **← channels：** `ChannelMonitor` 注册 plugin manifests；bridge factory 每次 respawn 后把 `PluginHost` 指向新的 runtime。
- **← agent：** `Agent::spawn` 以 policy `Never` 注册 ACP adapters。
- **← server：** daemon shutdown 调 `kill_all`；daemon start 调 `orphan_sweep`。
- **→ nothing above it：** 本模块是 leaf，不应知道 threads、routes 或 profiles。

## 不变量：不要破坏

1. **每次 spawn fresh bridge**：one-shot state 放在 bridge，不放在 factory closure；respawned process 不能看到前任状态。
2. **Supervisor 从不解释 pipe content**，协议关注点留在 bridges。
3. **单一 active generation record**：registry id、cancel sender、bridge task 必须随 generation id 一起移动；旧 bridge exit 只能清理自己的 child。
4. **双层清理**：正常 Stop 在返回前 cancel、tree-reap、join/有界 abort bridge；`ChildRegistry::kill_all` 仍是异常 teardown 的 safety net。
5. **所有 child 都用 enriched env**：通过 `process::env::command` spawn，让 PATH 和用户 shell 一致。
6. Heartbeat watchdog 只用于 plugins；agents 设计上 loud crash（`Never`），由 owning thread 决定是否重启。

## 已知技术债

- Unix descendants 已通过 process group 终止；Windows 仍需 Job Object。
- global Supervisor/Registry 仍妨碍测试隔离与 scoped dependency ownership，下一步应注入 scoped supervisor handle。
- Restart 已有有界指数退避，但仍缺 jitter、failure-window budget 与显式 circuit breaker/manual reset 状态。
- `Running` 目前只代表 child 与 bridge generation 已发布；channel protocol/platform readiness 仍需要独立 handshake/ready signal。

---

*Source anchors: `src/core/src/process/`（supervisor、model、generation、bridge、registry、kill）。*
*Last verified: `codex/im-acp-route-refactor` at `924d4c60`（2026-07-11）。*

<sub>[◀ Module: workspace](workspace.md) · [文档索引](../../README.md) · [Module: agent ▶](agent.md)</sub>
