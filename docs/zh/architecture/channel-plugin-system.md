# 渠道插件系统

每个 IM 集成 —— Telegram、Slack、飞书、Discord、微信、钉钉、企业微信、QQ 机器人 —— 都是一个**渠道插件**：一个独立的 Node.js 进程，一侧说平台的 API，另一侧用基于 ACP 的小协议和 VibeAround 守护进程通信。本页解释这套系统如何工作。要配置现有渠道，见[连接渠道](../guides/connect-channels.md)；要写新插件，见[开发渠道插件](../guides/build-a-channel-plugin.md)。

## 为什么用进程外插件

- **隔离：** 平台 SDK 崩溃或内存泄漏只杀死一个插件进程，不伤守护进程。监督器会重新拉起它。
- **生态匹配：** IM 平台 SDK 绝大多数是 JavaScript；插件跑在 Node.js 上，用 `@vibearound/plugin-channel-sdk` npm 包，守护进程保持 Rust。
- **独立发行：** 每个插件是自己的仓库和 npm 包，独立于守护进程发布节奏进行版本管理和更新。

有两个渠道是内置而非插件：`web`（控制台的 Web Chat）和 `tui` 在进程内跑同一套渠道接口，让消息路由无论来自哪个界面都走同一条代码路径。

## 插件放在哪、如何被发现

插件从 VibeAround 数据目录（`~/.vibearound/plugins/<id>/`）被发现，各带一份清单，声明 `kind: "channel"`、入口点和配置 schema。桌面引导流程会把插件包装到那里；开发时也会扫描项目本地的插件目录。

被发现的插件只有在 [`settings.json`](../reference/configuration.md#settingsjson) 的 `channels.<name>` 下有配置时才会**运行** —— 没配置就保持禁用。

## 进程生命周期

```text
注册 ──► 拉起 (node <entry>) ──► 运行中 ──► 崩溃 / 冻结
           ▲                                  │
           └──── 延迟后重新拉起 ◄─────────────┘
```

守护进程的进程监督器掌管每个插件进程：

- **崩溃重启。** 退出的插件按有上限的指数退避重新拉起。
- **心跳看门狗。** 插件每 30 秒发一次 `_va/heartbeat` 通知；90 秒没收到就认定插件冻结，杀掉并重启。这抓住了那些挂死但不退出的平台 SDK（数值见[计时器与上限](../reference/timers-and-limits.md#supervision)）。
- **只做实时输出。** IM 输出只经过一个小型有界内存缓冲；不会落盘，也不会在 plugin 或 daemon 重启后重放。连接已断开的投递会被丢弃并记录日志。
- **待决权限清理。** 插件在权限请求等待点按时死掉，待决请求会被取消，让 Agent 的回合快速失败而不是永远挂着。

生命周期也可以手动管理：`va channels`（列出）、`va channel start|stop|restart <instance_id>`、`va channel sync`（把运行中的插件与 `settings.json` 对齐），或桌面 UI 的等价控制。当前 legacy 单实例配置中，instance id 与 channel kind 相同。

## 线上协议，简述

插件 ↔ 守护进程的通信是 stdio 上的 JSON-RPC，使用 ACP 帧。重要的消息形状：

**入站（插件 → 守护进程）：** 渠道信封 —— route key（channel kind、稳定 channel instance id、actor id、chat id、可选 topic id）、消息 id、发送者、文本、附件 —— 或回调（带 action value 的按钮点按），或控制输入（stop、close）。

**出站（守护进程 → 插件）：** Agent 输出块、系统文本、回合状态（用于输入中指示）、prompt-done 标记，以及**权限请求** —— 携带 request id 和一个负载，由插件渲染成平台原生的交互卡片（飞书卡片用 V2 schema；Slack 用 block actions，等等）。插件用同一个 request id 把用户的选择发回来，回答权限请求。

附件按引用流转：插件把平台文件下载到共享缓存目录并传安全的 file key；守护进程把它们变成给 Agent 的资源链接。

## 身份与路由

`channel_kind` 决定使用哪一种 plugin 实现；`channel_instance_id` 是 host 持有的稳定生命周期/runtime 主键；`actor_id` 是平台上被点名的 bot/actor。完整 route 还包含 `chat_id` 与可选 `topic_id`，所以不同 actor/topic 可以附着到不同 workspace thread。消息只保证完整 route 内 FIFO，不保证平台全局顺序。

## 与插件仓库的关系

主仓库包含插件*宿主*（发现、监督、传输）。插件本身 —— 以及它们依赖的 `@vibearound/plugin-channel-sdk` 包 —— 在各自独立的仓库里，各有 README 覆盖平台侧准备（bot 注册、权限、webhook）。分工约定：本文档讲机制；按平台的配置步骤跟着各插件走。

---

*Source anchors: `src/core/src/plugins/` (discovery, manifest), `src/core/src/channels/` (transport_stdio, plugin_host, monitor), `src/core/src/process/supervisor.rs` (respawn, watchdog), `src/core/src/routing.rs` (RouteKey).*
*Last verified: `codex/im-acp-route-refactor`（2026-07-11）。*

<sub>[◀ 会话生命周期](session-lifecycle.md) · [文档索引](../README.md) · [本地 API 与 Bridge ▶](local-api-and-bridge.md)</sub>
