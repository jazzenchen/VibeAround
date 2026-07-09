# 计时器与上限

每一个超时、TTL、间隔和大小限制，集中一张表。**本页是这些数字的唯一权威** —— 其他页面链接到这里而不是复述；代码里的值变了，改这里，别处不改。

## 生命周期与 TTL

| 值 | 管什么 | 定义于 |
|---|---|---|
| 10 分钟 | 宿主 Agent 闲时关停 —— Agent 进程停止，Thread 保持开启，下一条提示恢复 | `src/core/src/workspace/manager.rs`（`AGENT_HOST_IDLE_SHUTDOWN_DELAY`） |
| 120 秒 | 交接接续码 TTL（4 字符码，一次性） | `src/core/src/workspace/handover.rs` |
| 60 秒 | 浏览器配对码 TTL（6 位码，可刷新） | `src/core/src/auth/pair.rs`（`CODE_TTL`） |
| 600 秒 | 预览**分享**链接寿命（owner 链接永不过期） | `src/core/src/previews/store.rs`（`SHARE_TTL_SECS`） |
| 每次守护进程启动 | 控制台认证 token 轮换 —— 每次重启让之前所有 URL 失效 | `src/core/src/auth/token.rs` |

## 监督

| 值 | 管什么 | 定义于 |
|---|---|---|
| 15 秒 | 渠道插件心跳节奏（`_va/heartbeat`） | 插件 SDK 契约 |
| 90 秒 | 插件看门狗窗口 —— 这么久没心跳 → 杀掉 + 重启 | `RestartPolicy::OnCrash { watchdog }` |
| 5 秒 | 监督器 tick —— 崩溃重启调度的粒度 | `src/core/src/process/supervisor.rs`（`TICK_INTERVAL`） |
| 从不 | Agent 进程不自动重启（`RestartPolicy::Never`）—— 崩溃呈报给所属 Thread | `src/core/src/agent/runtime.rs` |

## 大小与数量

| 值 | 管什么 | 定义于 |
|---|---|---|
| 64 MB | 本地 bridge 端点的最大请求体（大上下文负载） | `src/server/src/web_server/mod.rs`（`LOCAL_BRIDGE_BODY_LIMIT_BYTES`） |
| 64 | 渠道输入分片 worker 数 —— 同一 Route 严格有序，Route 之间并行 | `src/server/src/lib.rs`（`CHANNEL_INPUT_WORKER_COUNT`） |
| 4 字符 / 32 字符字母表 | 交接码格式 | `src/core/src/workspace/handover.rs` |
| 6 位数字 | 配对码格式 | `src/core/src/auth/pair.rs` |

## 网络默认值

| 值 | 管什么 | 定义于 |
|---|---|---|
| `12358` | 守护进程端口（HTTP、WS、MCP、bridge） | `src/core/src/config.rs`（`DEFAULT_PORT`） |
| `127.0.0.1` | 绑定地址；bridge 端点额外拒绝非回环调用方 | `src/server/src/` |
| 3 秒 | Web 监听优雅关停超时，超时则强制中止 | `src/server/src/lib.rs`（`WEB_SHUTDOWN_TIMEOUT`） |

权限请求刻意没有超时 —— Agent 的回合可以无限等人；终止性由取消路径保证（[权限流程](../internals/flows/permission.md)）。

---

*Source anchors: 上表"定义于"一列 —— 每行标注了对应常量。*
*Last verified: v0.7.11*

<sub>[◀ API 面参考](api-surfaces.md) · [文档索引](../README.md) · [供应商端点参考 ▶](provider-endpoints.md)</sub>
