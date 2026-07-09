# Module: pty

`src/core/src/pty/`：支撑 Web 终端的 pseudo-terminal sessions。流程：[PTY 终端](../flows/web-terminal.md)。

## 职责

通过 portable PTY（`portable_pty`）启动 shells 或 agent CLIs，维护 live sessions registry，并把它们的 byte streams 桥接给 attach 的界面。Detach-without-kill 是定义性行为：sessions 比 viewers 活得久，但不会比 daemon 活得久。

## 关键类型

| Type | File | Role |
|---|---|---|
| `Registry` (`new_registry`) | `mod.rs` | 以 id 索引 live sessions 的 DashMap |
| `PtySessionManager` | `manager` (behind `mod.rs`) | Create / attach / delete sessions；tool resolution（shell vs agent pty command） |
| PTY runtime | `runtime.rs` | pty 下的 child：spawn、resize、read/write pumps、`try_wait` polling → run-state events |

## 交互

- **← server (`ws_pty`)：** WebSocket attach/detach、resize messages、byte relay。
- **← cli：** `va session create/attach/kill`、`va tmux sessions`。
- **→ process::env：** 加强过的 login-shell environment；`resources::PTY_ENV` terminal defaults（清掉继承的 `NO_COLOR`/`TERM=dumb`）。
- **→ resources：** agent `pty.command` strings；desktop-only agents 被拒绝。

## 不变量：不要破坏

1. **Detach ≠ kill**：丢掉 WebSocket 必须让 child 继续运行；只有显式 delete 才 kill。
2. **Sessions 随 daemon 死亡**：`RunningDaemon::stop` 删除所有 sessions，PTY children 绝不 orphan。
3. **Env 要和真实终端一致**：用 enriched env spawn，否则 agent CLIs 会莫名其妙缺 PATH entries。
4. Exit 通过轮询 `try_wait` 检测，并推送成 run state；attach views 必须总能知道 child 已死。

## 已知技术债

- remediation plan 中无跟踪项；模块小且稳定。

---

*Source anchors: `src/core/src/pty/` (mod, runtime, manager, session), `src/server/src/web_server/ws_pty.rs`, `src/server/src/lib.rs` (shutdown deletion).*
*Last verified: v0.7.11*

<sub>[◀ Module: profiles](profiles.md) · [文档索引](../../README.md) · [Module: previews ▶](previews.md)</sub>
