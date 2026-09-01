# Internals

用于调试 VibeAround 和修改代码的文档。同一个运行时有两种互补的切面：

- **Flows** 跟着一个请求按时间往前走：从入口到出口的每一跳，带代码锚点和失败表。追踪行为时从这里开始。
- **Modules** 把一个组件按空间展开：职责、关键类型、交互关系、不能破坏的不变量，以及已知技术债。改代码时从这里开始。

Flow 经过某个模块的地方会互相交叉链接。面向读者的“为什么这样设计”在[架构](../architecture/overview.md)里；硬数字在[计时器与上限](../reference/timers-and-limits.md)里。

## Flows

| Flow | 路径 |
|---|---|
| [IM 消息](flows/im-message.md) | 平台事件 → 插件 runner → 有界 route lane → Thread → Agent → 流式回复。**主干流程，优先读这个** |
| [Web Chat](flows/web-chat.md) | WebSocket 事件 → 会话意图 → 同一条 prompt 路径 |
| [权限请求](flows/permission.md) | Agent 请求 → oneshot 注册 → 卡片 → 点按 → Agent 继续 |
| [Bridge 请求](flows/bridge-request.md) | 客户端方言 → 解码 → 模型映射 → 上游 → 流式返回 |
| [原生启动](flows/native-launch.md) | Profile → launch JSON → va-launch → 终端启动 |
| [交接](flows/handover.md) | 签发短码 → `/pickup` → 外部 session 绑定 → route 附着 |

## Modules

每页固定结构：职责 · 关键类型 · 交互关系 · 不变量 · 已知技术债。

| Module | 一句话 |
|---|---|
| [channels](modules/channels.md) | 各界面与 Thread 之间的消息传输和路由 |
| [workspace](modules/workspace.md) | 对话状态：workspace、thread、attachment，事件溯源 |
| [process](modules/process.md) | 子进程监管：启动、重启、watchdog、清理 |
| [agent](modules/agent.md) | 到一个编程 CLI 的 ACP 连接，以及启动准备 |
| [profiles](modules/profiles.md) | Provider catalog、profile 存储、启动渲染 |
| [previews](modules/previews.md) | Server/Markdown owner 与 Share URL、受限 Server Share 代理 |
| [tunnels](modules/tunnels.md) | ngrok / localtunnel / cloudflare / Tailscale Funnel 发布 |
| [auth](modules/auth.md) | Daemon token 和配对码 |
| [server](modules/server.md) | axum 外壳：routes、WebSockets、MCP、bridge、启动/关闭 |

## 子系统深挖

跨多个模块的横切子系统有单独页面：

| 页面 | 覆盖内容 |
|---|---|
| [Launch](launch.md) | 三条启动路径、每条路径的 env 组装与注入、各 OS 终端处理、参数来源、desktop 与 CLI producer 差异 |

## 相关材料

- 已知缺陷与计划重构由各模块页“已知技术债”和 `reports/system-review-2026-07-10/` 三份当前报告共同维护。
- 源码里的 Rustdoc module header 是最细粒度的权威；这些页面是地图，不替代源码。

<sub>[◀ 供应商端点参考](../reference/provider-endpoints.md) · [文档索引](../README.md) · [Flow: IM 消息 ▶](flows/im-message.md)</sub>
