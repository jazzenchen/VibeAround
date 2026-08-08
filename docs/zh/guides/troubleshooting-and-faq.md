# 故障排查与 FAQ

先看症状，再讲概念。拿不准就先跑 `va doctor` —— 它一次性检查端点、认证和服务健康。

## 连接与认证

**重启后控制台显示 401 / 要求认证。**
正常现象：认证 token 在守护进程每次启动时重新生成。从受信入口重新打开（桌面托盘 → Dashboard，或 `va status` 给出的 URL）；远程浏览器需要重新配对。

**我启动的 Agent CLI 显示 `Unable to connect to API (ConnectionRefused)`。**
这个 CLI 是用 Bridge 化的 Profile 启动的，而守护进程没在运行 —— `127.0.0.1:12358` 的 Bridge 就是它的模型端点。启动桌面应用或 `va serve`，然后在 CLI 里重试。

**端口 12358 被占用。**
另一个 VibeAround 实例（或崩溃留下的残留进程）占着它。`va status` 能告诉你是否有健康的守护进程在应答；否则找到并杀掉占用者。守护进程启动时会清扫孤儿子进程，但上一个*守护进程*本身要由你来停。

**配对码总是无效或过期。**
码只活 60 秒。生成后立刻在连着同一个守护进程的聊天里确认（`/pair <code>`）或在本机确认。机器 A 的码在机器 B 的守护进程上永远无效。

## 渠道

**插件显示崩溃 / 不停重启。**
几乎总是凭据或平台配置问题。查守护进程日志（插件的 stderr 带渠道 kind 标签），修好 `channels.<kind>`，然后 `va channel restart <kind>`。

**bot 完全不响应。**
`va channels` —— 插件在运行吗？渠道配置了吗（没有 `channels.<kind>` 配置的插件保持禁用）？平台侧：webhook URL 可达 / 长轮询启动了吗？然后看聊天里 `/status` 是否有响应 —— 命令能用但提示不行，就去查 Agent 错误。

**权限卡片上的按钮点了没反应。**
更新插件 —— action 回调需要较新的插件 + SDK。飞书专属问题：卡片必须用 V2 卡片 schema，平台不支持 V1 的 action 标签。

**消息乱序 / 不同聊天交错。**
单个聊天内的处理是严格有序的。跨聊天并行是设计使然。如果单个聊天出现交错，多半是一个群里有两个 bot（两条 Route）—— 每个 bot 维护自己的 Thread。

## Agent 与 Thread

**"Workspace thread is closed" 错误。**
Thread 被关了（`/close`，或不可恢复的 Agent 错误后自动关闭）。发 `/new` 继续。

**Agent 回合中途不回话了。**
先停止回合（停止按钮 / 插件的停止命令），查 `/status`。如果 Agent 进程崩了，下一条提示会拉起新进程并恢复 Session。一拉起就持续失败通常是 Agent CLI 缺失或需要登录 —— 手动启动一次试试。

**`/switch` 到另一个 Agent 后上下文没了。**
正常现象：切换到不同的 Agent 会创建带全新 Session 的新 Thread —— 上下文不会在不同 Agent 产品之间传递。旧对话还在：`/session` 列出，`/session --switch <id>` 重新附着。只切 Profile（同一个 Agent）则保留 Session。见[会话生命周期](../architecture/session-lifecycle.md)。

**Agent 报 Authentication required。**
Agent CLI 自己需要官方登录（`claude login` 等）—— VibeAround 托管它，但不能替你登录。这类错误会让 Thread 自动关闭；在终端里登录后 `/new`。

**对话能挺过守护进程重启吗？**
在要紧的意义上，能：Thread、Route 附着关系和 CLI session id 都被持久化，Agent 的对话记录存在 Agent 自己的存储里。进行到一半的回合会丢失。旧的"会话只存在于内存"的限制已经不存在了。

## Session、交接、预览

**`/pickup` 说码无效。**
交接码一次性、短寿命 —— 重新发一个并立刻用。两个界面必须连着同一个守护进程。

**`/session --switch` 找不到我的终端会话。**
`va launch sessions` 显示发现机制能看到什么。会话必须属于聊天绑定的同一个 Agent 和 Workspace；已归档的会话是隐藏的（`va launch unarchive` 取消归档）。

**Markdown 预览分享链接几分钟后失效了。**
每笔 Markdown 分享的 URL、六位访问码和浏览器授信设计上共用 600 秒期限。重新分享可创建一笔新事务，或者使用你已认证的 owner 链接。Live Server 预览仅限本机。见[安全模型](../architecture/security-model.md)。

## 模型与 Profile

**Agent 拒绝模型名。**
用 Profile 的别名模型 id（Agent 常按厂商列表校验模型名）。[模型 Profile 指南](model-profiles.md#profile-故障排查)有关于 401、能力不匹配和限流的更完整表格。

## 平台差异

- **macOS Intel：** 桌面应用只能源码构建（[源码构建](build-from-source.md)）。
- **Linux 终端启动**依赖桌面环境；VibeAround 依次尝试 `xdg-terminal-exec`、`x-terminal-emulator` 和常见终端（GNOME Terminal、Konsole、XFCE Terminal、xterm、Kitty、Alacritty、WezTerm）。
- **Windows 重启抖动**（重启后立刻 "port busy"）会自动重试；持续出现说明确实有旧进程活着。

## 日志在哪？

守护进程日志写在 `~/.vibearound/logs/runtime/vibearound.log.<date>`（按日滚动，无 ANSI 转义），并镜像到 stdout / 桌面应用的日志视图。插件 stderr 内嵌在守护进程日志里，带渠道 kind 标签。

过滤用标准的 `RUST_LOG` 环境变量；默认是 `info,common=debug`（运行时详细、依赖安静）。调试会话示例：

```bash
RUST_LOG=debug va serve                      # 全部 debug
RUST_LOG=warn,common::channels=trace va serve  # 只 trace 一个子系统
```

报告问题时，`va doctor` 输出加上当前日志文件的相关片段就是有用的最小集。

---

*Source anchors: `src/core/src/auth/pair.rs` (code TTL), `src/core/src/channels/prompt/mod.rs` (auto-close reasons), `src/core/src/workspace/` (persistence), `src/core/src/previews/store.rs` (share TTL), `src/server/src/lib.rs` (orphan sweep, Windows bind retry), `src/core/src/logging.rs` (log destinations).*
*Last verified: v0.7.11*

<sub>[◀ 源码构建](build-from-source.md) · [文档索引](../README.md) · [核心概念 ▶](../architecture/concepts.md)</sub>
