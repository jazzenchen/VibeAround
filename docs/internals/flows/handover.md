# Flow: handover

How a conversation moves from one surface to another with context intact. The trick is that nothing conversational actually moves: a short-lived code carries *pointers* (agent, session id, workspace), and the receiving surface re-binds to the same CLI session.

## Hop by hop

```text
terminal CLI ──1─► MCP prepare_handover ──2─► code stored (4 chars · 120 s · one-shot)
                                                   │
phone IM chat ──3─ /pickup CODE ──4─► consume ──5─► attach external session to thread
                                                   │6
                                            route attached · agent respawned · session resumed
```

**1. The request.** Inside a launched agent CLI, the user invokes the handover skill (`/vibearound handover`); the agent calls the `prepare_handover` MCP tool on the daemon, passing its identity (resolved via `get_session_id` from the injected `VIBEAROUND_*` env / session context).
→ `src/skills/vibearound/`, `src/server/src/web_server/mcp/tools.rs`

**2. Code issued.** The daemon stores `{agent_kind, profile_id, session_id, cwd}` under a 4-character code (32-char alphabet, OS RNG). TTL 120 seconds, one-shot consumption, expired entries purged on access.
→ `src/core/src/workspace/handover.rs`

**3. Pickup typed.** `/pickup K7PQ` in any connected chat parses as a thread command on that chat's route.
→ `src/core/src/channels/prompt/handler.rs` (`ThreadCommand::Pickup`)

**4. Consume.** The code is atomically removed from the table — a second `/pickup` with the same code fails cleanly ("invalid or expired").

**5. Bind the external session.** `attach_external_session` resolves the payload into a thread: ensure a workspace for the cwd, resolve the session id against native session discovery (alias/prefix tolerant), and either reuse an open thread already bound to that session or create one recording the host binding + session.
→ `src/core/src/workspace/manager.rs` (`attach_external_session`, `prepare_external_session_thread`)

**6. Attach and resume.** The chat's route is attached to that thread. The next prompt (or the startup notification) spawns the agent **in the recorded workspace** and resumes the recorded CLI session — the agent wakes up mid-conversation. Because attachment is additive, the terminal's history is now also this chat's history, and if another surface attaches later, output fans out to all of them.

## Variants on the same mechanism

| Variant | Difference |
|---|---|
| Web → phone | The dashboard issues the code for a web thread's session; pickup is identical |
| `/session --switch <id>` in chat | Skips the code: directly binds a discovered native session to the chat's route |
| Web chat "Resume" picker | Same binding, driven by a `ResumeSession` intent over `/ws/chat` instead of a command |

All three converge on `attach_external_session` — one binding path, three doorways.

## Sharp edges

- **The code is bearer-like:** anyone who can message your bot within 120 s can claim it. The window and one-shot semantics are the mitigations; treat codes like short-lived secrets.
- **Cross-agent pickup is not a thing:** the code pins the agent kind; picking up a Codex session resumes Codex, regardless of the chat's previous host.
- **The cwd matters:** resume happens in the recorded workspace. If the directory is gone, agent spawn fails with a clear error rather than resuming somewhere wrong.

---

*Source anchors: `src/core/src/workspace/handover.rs` (codes), `src/server/src/web_server/mcp/tools.rs` (prepare_handover, get_session_id), `src/core/src/channels/prompt/handler.rs` (pickup), `src/core/src/workspace/manager.rs` (attach_external_session), `src/core/src/launch_sessions/` (session resolution).*
*Last verified: v0.7.11*

<sub>[◀ Flow: agent launch](native-launch.md) · [Documentation index](../../README.md) · [Flow: PTY terminal ▶](web-terminal.md)</sub>
