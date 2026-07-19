# IM 使用

如何在聊天窗口里驱动编程 Agent，以及完整的斜杠命令参考。本页适用于所有渠道 —— Telegram、Slack、飞书、Discord、微信、钉钉、企业微信、QQ 机器人 —— 以及理解同一套命令的内置 Web Chat。

## 基础

任何不是命令的消息，都是发给托管该聊天 Thread 的 Agent 的提示。聊天里的第一条消息会自动在默认 Workspace 创建 Thread。输出边产生边流回；长时间的工具运行会显示进度而不是沉默。

**权限卡片。** Agent 需要批准时（运行命令、在允许范围之外编辑），聊天会收到一张带选项的交互卡片。你点按之前 Agent 会一直等着。卡片在插件重启后仍然有效 —— 未回应的卡片会被重新投递。

**附件。** 你发送的文件会被下载并作为文件引用交给 Agent；Agent 可以在其 Workspace 上下文中读取它们。图片对接受图像输入的 Agent/模型有效。

**群聊。** 每个（bot × 聊天）组合是独立的 Route，拥有自己的 Thread，所以群里的讨论和你与同一个 bot 的私聊是两个互不相干的对话。一个群里的多个 bot 也各自维护自己的 Thread。

## 命令参考

命令支持两种前缀风格：`/command`，或 `/va command` / `va command`（在保留斜杠命令的平台上很有用）。`/vibearound` 是 `/va` 的别名。

### Thread 控制

| 命令 | 效果 |
|---|---|
| `/new` | 关闭当前 Thread，使用该渠道配置的 Agent/Profile 在同一 Workspace 开一个新的 |
| `/close` | 关闭 Thread；下一条消息会开始新的 |
| `/status` | 显示 thread id、Workspace、宿主 Agent、Profile、Session、忙闲状态 |
| `/help`（或 `/commands`、`/va`） | 显示命令帮助 |

### 停止工作

取消进行中的回合是渠道层面的信号，不是宿主斜杠命令：Web Chat 有停止按钮，每个 IM 插件有自己的方式（停止命令或按钮 —— 见插件的 README）。即使 Agent 正忙，信号也会被送达。

### Workspace

| 命令 | 效果 |
|---|---|
| `/workspace` | 列出已注册的 Workspace，并标出当前所在 |
| `/workspace --switch <id-or-name>` | 把这个聊天移到另一个 Workspace（在那里开一个 Thread） |
| `/switch workspace <token>` | 同上，另一种写法 |

### Agent 与 Profile

| 命令 | 效果 |
|---|---|
| `/agent`（或 `/agent --list`） | 列出已启用的 Agent |
| `/agent --switch <id>` | 切换这个 Thread 的宿主 Agent |
| `/switch <agent>` | 宿主切换的简写，如 `/switch codex` |
| `/switch host <agent> [profile]` | 切换宿主并显式指定 Profile |
| `/switch <agent>+<profile>` | 同上的紧凑形式，如 `/switch claude+moonshot` |
| `/profile`（或 `/profile --list`） | 列出模型 Profile |
| `/profile --switch <id>` | 把 Thread 重新绑定到另一个 Profile |
| `/agent <其他任何内容>` | 作为原生命令透传给托管的 Agent（如 `/agent compact`） |

切换到不同的 Agent 会创建带全新 Session 的新 Thread；只切换 Profile 则保留当前 Thread 和 Session。见[会话生命周期](../architecture/session-lifecycle.md)。

### Session 与连续性

| 命令 | 效果 |
|---|---|
| `/session`（或 `/session --list`） | 列出此 Workspace/Agent 下可恢复的 Session |
| `/session --switch <id>` | 把这个聊天附着到已有 Session（支持前缀匹配） |
| `/pickup <code>` | 接续从终端或 Web 会话发出的交接码 |
| `/pair <code>` | 确认浏览器配对码，用于隧道化控制台访问 |

### 未知命令

以 `/` 开头但解析失败的内容会被报告为未知命令，而不是发给 Agent —— 打错字不会意外变成提示。

## 实用模式

- **发完就走：** 启动一个长任务，把手机揣兜里；Agent 需要决定时权限卡片会提醒你，完成后回复自动流回来。
- **两个 Agent 并行：** 在一个聊天里 `/switch codex`，另一个聊天保持 Claude —— 不同 Thread，回合互不影响。
- **拯救卡住的回合：** 先停掉（停止按钮或插件的停止命令），`/status` 确认空闲，再重新提示。如果 Thread 本身卡死了，`/new` 在同一 Workspace 干净重来。
- **继续昨晚的终端会话：** `/session` 找到它，`/session --switch <id>` 附着上去 —— 对 VibeAround 之外创建的会话同样有效。

---

*Source anchors: `src/core/src/channels/prompt/handler.rs` (parse_thread_command — the command grammar above mirrors it 1:1), `src/core/src/channels/prompt/mod.rs` (attachments, callbacks), `src/core/src/channels/types.rs` (Stop/Close inputs).*
*Last verified: v0.7.11*

<sub>[◀ Web 控制台指南](web-dashboard.md) · [文档索引](../README.md) · [连接渠道 ▶](connect-channels.md)</sub>
