# Flow: 交接

一次对话如何带着上下文从一个界面移动到另一个界面。诀窍是对话内容其实没有移动：短寿命码只携带*指针*（agent、session id、workspace），接收界面重新绑定到同一个 CLI session。

## 逐跳

```text
terminal CLI ──1─► MCP prepare_handover ──2─► code stored (4 chars · 120 s · one-shot)
                                                   │
phone IM chat ──3─ /pickup CODE ──4─► consume ──5─► attach external session to thread
                                                   │6
                                            route attached · agent respawned · session resumed
```

**1. 请求。** 在 launched agent CLI 里，用户调用 handover skill（`/vibearound handover`）；agent 调 daemon 上的 `prepare_handover` MCP tool，传入自己的 identity（通过注入的 `VIBEAROUND_*` env / session context，由 `get_session_id` 解析）。
→ `src/skills/vibearound/`, `src/server/src/web_server/mcp/tools.rs`

**2. 签发短码。** Daemon 把 `{agent_kind, profile_id, session_id, cwd}` 存到一个 4 字符 code 下（32 字符 alphabet，OS RNG）。TTL 120 秒，一次性消费，过期 entry 在访问时 purge。
→ `src/core/src/workspace/handover.rs`

**3. 输入 pickup。** 任意已连接聊天里的 `/pickup K7PQ` 会解析为该聊天 route 上的 thread command。
→ `src/core/src/channels/prompt/handler.rs` (`ThreadCommand::Pickup`)

**4. Consume。** Code 会从表里原子移除；第二次用同一个 code `/pickup` 会干净失败（“invalid or expired”）。

**5. 绑定外部 session。** `attach_external_session` 把 payload 解析成一个 thread：确保 cwd 对应的 workspace 存在，对照 native session discovery 解析 session id（容忍 alias/prefix），然后要么复用一个已经绑定到该 session 的 open thread，要么创建一个记录 host binding + session 的新 thread。
→ `src/core/src/workspace/manager.rs` (`attach_external_session`, `prepare_external_session_thread`)

**6. Attach and resume。** 该聊天的 route 附着到那个 thread。下一条 prompt（或 startup notification）会在**记录的 workspace**里 spawn agent，并 resume 记录的 CLI session，agent 会在对话中途醒来。因为 attachment 是增量的，终端历史现在也是这个聊天的历史；如果稍后还有另一个界面附着，output 会 fan out 到所有界面。

## 同一机制的变体

| 变体 | 差异 |
|---|---|
| Web → phone | Dashboard 为 web thread 的 session 签发 code；pickup 完全相同 |
| 聊天里的 `/session --switch <id>` | 跳过 code：直接把发现到的 native session 绑定到聊天 route |
| Web chat “Resume” picker | 同样的 binding，但由 `/va/ws/chat` 上的 `ResumeSession` intent 驱动，而不是命令 |

三者最终都汇到 `attach_external_session`：同一条 binding 路径，三个入口。

## 锐边

- **Code 类似 bearer token：** 120 秒内，任何能给你的 bot 发消息的人都能领取它。窗口短、一次性，是这里的缓解措施；把 code 当短寿命 secret 对待。
- **没有 cross-agent pickup：** code 固定 agent kind；领取 Codex session 就 resume Codex，不管聊天原来的 host 是谁。
- **cwd 很重要：** resume 发生在记录的 workspace。目录没了，agent spawn 会明确失败，而不是在错误位置 resume。

---

*Source anchors: `src/core/src/workspace/handover.rs` (codes), `src/server/src/web_server/mcp/tools.rs` (prepare_handover, get_session_id), `src/core/src/channels/prompt/handler.rs` (pickup), `src/core/src/workspace/manager.rs` (attach_external_session), `src/core/src/launch_sessions/` (session resolution).*
*Last verified: v0.7.11*

<sub>[◀ Flow: Agent 启动](native-launch.md) · [文档索引](../../README.md) · [Flow: PTY 终端 ▶](web-terminal.md)</sub>
