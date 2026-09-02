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
| `Lease` | `lease.rs` | 由内核持有的保证：子进程绝不活过 daemon。Unix：绑在管道上的 `sh` reaper；supervisor 在 spawn 成功后立刻写 `add <pgid>`，回收后写 `del <pgid>`，管道一关 reaper 就杀掉名单上剩下的。刻意不覆盖的两点：fork 到写入 `add` 之间约 1 毫秒（daemon 恰在此刻被杀会漏掉那一个子进程——那要么是该修的 bug，要么是用户自己动的手），以及 reaper 被人手动杀掉（此后新子进程不受保护，日志有警告）。Windows（过渡）：沿用原来的 pid 花名册，由 exit handler 杀 |
| `orphan_sweep` | `orphan.rs`（仅 Windows） | 启动时清理上一次 daemon 崩溃残留的子进程，直到 Windows 的 lease 换成真正的 Job Object |
| `AcpTransport` wrapper | `acp_transport.rs` | ACP line transport + 显式 EOF signal，让 supervisor 观察 child death |
| `env` | `env.rs` | 加强过的 login-shell environment（缓存一次），注入每个 child |

## 交互

- **← channels：** `ChannelMonitor` 注册 plugin manifests；bridge factory 每次 respawn 后把 `PluginHost` 指向新的 runtime。
- **← agent：** `Agent::spawn` 以 policy `Never` 注册 ACP adapters。
- **← server：** daemon shutdown 调 `shutdown_all`（同进程内热重启也走这里，OS 级 lease 在那种情况下不会触发）；daemon start 只在 Windows 上跑 `orphan_sweep`。
- **→ nothing above it：** 本模块是 leaf，不应知道 threads、routes 或 profiles。

## 不变量：不要破坏

1. **每次 spawn fresh bridge**：one-shot state 放在 bridge，不放在 factory closure；respawned process 不能看到前任状态。
2. **Supervisor 从不解释 pipe content**，协议关注点留在 bridges。
3. **单一 active generation record**：registry id、cancel sender、bridge task 必须随 generation id 一起移动；旧 bridge exit 只能清理自己的 child。
4. **双层清理**：正常 Stop 在返回前 cancel、tree-reap、join/有界 abort bridge；daemon 以其他任何方式死亡都由 process lease（`lease.rs`）兜底：daemon 退出时内核关闭 lease，租约内的进程组被终止，不需要 daemon 这边跑任何代码。
5. **所有 child 都用 enriched env**：通过 `process::env::command` spawn，让 PATH 和用户 shell 一致。
6. Heartbeat watchdog 只用于 plugins；agents 设计上 loud crash（`Never`），由 owning thread 决定是否重启。

## 已知技术债

- Windows 还没有内核持有的 lease：过渡花名册只覆盖 exit handler，孤儿清扫只在下次启动时跑。正解是 kill-on-close 的 Job Object（在创建进程时原子加入，如 `PROC_THREAD_ATTRIBUTE_JOB_LIST`，或把 daemon 自己放进 job），需在 Windows 机器上实现并验证；届时一并删掉花名册和 `orphan.rs`。
- 刻意不纳入租约的：安装类的一次性子进程（`spawn_tree_killable`：npm install、startkit 脚本）和桌面端 onboarding 的 auth 脚本（短命、用户在场）。
- global Supervisor/Registry 仍妨碍测试隔离与 scoped dependency ownership，下一步应注入 scoped supervisor handle。
- Restart 已有有界指数退避，但仍缺 jitter、failure-window budget 与显式 circuit breaker/manual reset 状态。
- `Running` 目前只代表 child 与 bridge generation 已发布；channel protocol/platform readiness 仍需要独立 handshake/ready signal。

---

*Source anchors: `src/core/src/process/`（supervisor、model、generation、bridge、lease、kill）。*
*Last verified: `refactor/supervisor-lease`（2026-09-02）。*

<sub>[◀ Module: workspace](workspace.md) · [文档索引](../../README.md) · [Module: agent ▶](agent.md)</sub>
