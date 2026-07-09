# Flow: PTY 终端

Web 终端路径：浏览器 xterm 和你机器上的真实 pseudo-terminal 通信。这个流程中间刻意很笨，只做 bytes in、bytes out；智能在边缘，也就是 xterm.js 渲染和 PTY 生命周期管理。

## 逐跳

```text
browser xterm ◄──WS /ws?session_id=──► ws_pty handler ◄──channels──► PTY runtime ◄──pty──► shell / agent CLI
```

**1. Session creation。** 一个 terminal tab（或 `va session create --tool <tool>`）创建 PTY session：registry entry + session id，以及 pseudo-terminal 下的 spawned child。Tool 可以是 login shell（Unix 上 `bash -l`，Windows 上 `cmd.exe`），也可以是 registry 中 agent CLI 的 PTY command（`claude code --permission-mode acceptEdits`、`codex` 等）。Desktop-only agents 在这里会被拒绝，因为它们没有 CLI。
→ `src/core/src/pty/` (registry, runtime), `src/resources/agents.json` (`pty.command`)

**2. Environment。** Child 得到加强过的 login-shell environment（让 PATH 和真实终端一致）、terminal defaults（支持颜色的 TERM；清掉 GUI parent 继承来的 `NO_COLOR`/`TERM=dumb`）、theme hints，以及每个 session 的 extras。
→ `src/core/src/pty/runtime.rs` (`set_pty_env`), `src/core/src/process/env.rs`

**3. Attach。** 浏览器打开 `/ws?session_id=<id>`（token-authenticated）。从这里开始就是 byte pipe：PTY output → WS to client；WS input → PTY stdin。有一个 JSON 例外走 text frames：xterm-fit 重新计算后发送 `{"type":"resize","cols":…,"rows":…}`，用来 resize pseudo-terminal。
→ `src/server/src/web_server/ws_pty.rs`, `mod.rs` (`ResizeMessage`)

**4. Detach vs kill。** 关闭 tab 会断开 WebSocket，但**不会**杀 session。Child 继续运行，output 继续 buffer，稍后 attach 会恢复视图（`va session attach <id>` 从真实终端也一样）。显式 kill（`va session kill`、dashboard close、`va pty kill`）会终止 child 并删除 registry entry。

> 已知缺口：输出突发时，如果 client 落后超过 256 条 broadcast message，会被断开而不是 resync；scrollback dump 和 live subscription 之间发出的 bytes 会丢失。remediation plan 中以 M16 跟踪。

**5. Exit propagation。** Runtime 轮询 child；进程退出时把 run state 推给 frontend，让 tab 显示“process exited”，而不是冻结画面。

**6. Daemon shutdown。** Daemon stop 时删除所有 PTY sessions。PTY children 不会在 daemon 外继续存在（和 launched terminals 不同，后者属于你）。
→ `src/server/src/lib.rs` (`RunningDaemon::stop`)

## tmux attach 变体

安装 tmux 后，dashboard 可以 attach 到已有 tmux session，而不是 spawn 新 shell。此时 PTY child 是 `tmux attach`，`tmux.detach_others` setting 决定是否踢掉其它 clients。流程其余部分完全相同。
→ `src/core/src/pty/`, `src/cli` (`va tmux sessions`)

## 和其它执行路径的关系

| 路径 | 进程归属 | 协议 | Daemon stop 时杀掉 |
|---|---|---|---|
| PTY session（本流程） | daemon | raw bytes over WS | 是 |
| Hosted agent（[IM flow](im-message.md)） | daemon（supervisor） | ACP over stdio | 是 |
| Launched CLI（[launch flow](native-launch.md)） | 你的终端 | n/a（独立） | 否 |

运行 agent CLI 的 PTY session 是那个 CLI 的*终端*视图。它的 sessions 是 native sessions，和其它 launched CLI 一样可被发现、可交接。

---

*Source anchors: `src/core/src/pty/` (registry, runtime, session), `src/server/src/web_server/ws_pty.rs` + `mod.rs` (WS handler, resize), `src/core/src/process/env.rs` (enriched env), `src/resources/agents.json` (pty commands), `src/server/src/lib.rs` (shutdown).*
*Last verified: v0.7.11*

<sub>[◀ Flow: 交接](handover.md) · [文档索引](../../README.md) · [Module: channels ▶](../modules/channels.md)</sub>
