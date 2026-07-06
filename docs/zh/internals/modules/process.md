# Module: process

`src/core/src/process/`：daemon 拥有的每个子进程都经过这里，包括 spawn、supervision、restart policy、watchdog 和兜底清理。只要 fork 了，本模块就负责确保它正确死亡。

## 职责

为 child processes（channel plugins、agent ACP adapters）提供唯一的 supervised path，让 spawn/restart/cleanup 逻辑只存在一份。Supervisor 对 child pipes 上传输的协议**一无所知**，协议是 bridge 的责任，由所属模块提供。

## 关键类型

| Type | File | Role |
|---|---|---|
| `Supervisor` | `supervisor.rs` | 拥有 process lifecycles：状态机（NotStarted→Spawning→Running→Crashed→…）、5 秒 tick loop、restart policies、status broadcast |
| `SpawnSpec` | `supervisor.rs` | Program + args + cwd + env recipe，每次 respawn 复用 |
| `RestartPolicy` | `supervisor.rs` | `Never`（agents、PTY）或 `OnCrash { delay, watchdog }`（plugins，90 秒 heartbeat watchdog） |
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
3. **双层清理**：graceful path（cancel + drop）属于 supervisor；`ChildRegistry::kill_all` 是 runtime 先 teardown 时的同步 safety net。两者都要保留。
4. **所有 child 都用 enriched env**：通过 `process::env::command` spawn，让 PATH 和用户 shell 一致；绕过它会制造“终端可用，app 里失败”的 bug。
5. Heartbeat watchdog 只用于 plugins；agents 设计上要 loud crash（`Never`），由 owning thread 决定是否重启。

## 已知技术债

- `Supervisor::global()` 和 `ChildRegistry::global()` 单例妨碍测试隔离。计划：supervisor-tree 重构把 registry 吸收到 supervisor，并注入 `Arc<Supervisor>`（remediation M5 + 已锁定的 supervisor-tree 方向；OS descendants 通过 pgid cascade）。

---

*Source anchors: `src/core/src/process/` (supervisor, bridge, registry, acp_transport, env, kill, log), `reports/architecture-review-remediation-2026-07-04.md` (M5, §3).*
*Last verified: v0.7.11*

<sub>[◀ Module: workspace](workspace.md) · [文档索引](../../README.md) · [Module: agent ▶](agent.md)</sub>
