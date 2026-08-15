# Session lifecycle

This page answers the operational questions: when does a conversation start and end, what happens on restart, and what exactly moves when you hand a session over or switch agents. Vocabulary is defined in [Concepts](concepts.md).

## Thread lifecycle

A thread is born the first time a route needs one — the first message in a chat, or an explicit `/new` — and stays **open** until something closes it:

| Event | Effect |
|---|---|
| `/new` | Closes the current thread, creates a fresh one in the same workspace, re-attaches the route; IM routes reload the channel's configured agent/profile |
| `/close` | Closes the thread; the next message will create a new one |
| Unrecoverable agent error (e.g. authentication required) | Thread auto-closes with the reason sent to the chat |
| Daemon shutdown | Threads stay open — thread state is an on-disk event log |

Closed threads keep their history in the event log; they are never silently deleted.

## Agent process lifecycle within a thread

The agent process hosting a thread can be evicted under pool pressure, while the thread itself remains durable:

```text
first prompt ──► spawn host ──► create/resume CLI session ──► turn ──► warm host
                                                                         ├──► next prompt reuses live host
                     new host starts above soft limit + eligible LRU ◄───┤
                                                                         ▼
same ThreadRuntime + session ◄── host evicted ──► next prompt resumes session
```

- **No fixed idle shutdown:** finishing a turn, receiving `TurnStatus { active: false }`, or closing a Web tab does not start a process-kill deadline.
- **Pressure eviction:** only after a genuinely new host successfully starts above the warm-thread pool's [soft limit](../reference/timers-and-limits.md#sizes-and-counts) does the manager consider one least-recently-active candidate. It must meet the idle-age threshold, not be busy or the new thread, and have no resident subagents. If none qualifies, the pool may overflow.
- **Preserved continuity:** eviction stops only the host generation. The existing `ThreadRuntime`, thread/session records, route attachments, and preview records remain. The next prompt uses that retained runtime to spawn the host and resume the recorded CLI session.
- **Crash:** agent processes are not auto-respawned mid-turn (restart policy is deliberate: crashes surface as errors instead of silently retrying). The next prompt starts a fresh process and resumes the session.

## What survives a daemon restart

| Thing | Survives? | Notes |
|---|---|---|
| Open threads and their route attachments | Yes | Rebuilt from event logs at startup |
| CLI session ids observed per thread | Yes | Stored in thread events |
| Conversation context inside a session | Yes | Owned by the agent CLI's own storage; restored via resume |
| In-flight turn | No | A turn interrupted by restart is lost; the session resumes at its last completed state |
| Web chat scrollback in the browser | Partially | Startup replay re-sends recent output for web routes |
| Preview registrations | No | File and Server Previews exist only in the current daemon's in-memory registry |

## Handover: moving a conversation between surfaces

Handover attaches a second route to an existing thread, or re-binds an external CLI session into a thread:

1. **Terminal → IM.** Inside a launched agent CLI, the VibeAround MCP tool `prepare_handover` issues a short-lived code. Typing `/pickup <code>` in any connected IM attaches that chat's route to a thread bound to the same agent, workspace, and CLI session — the agent resumes with full context.
2. **Web → phone.** The same mechanism backs the dashboard's handover flow: the web thread's session is picked up by an IM route.
3. **Multiple listeners.** Because attachment is additive, output fans out to every attached route: you can watch the same turn in the web dashboard and in Telegram simultaneously.

Pickup codes are one-shot, expire quickly, and live **in memory only** — a daemon restart clears them, so re-issue the handover if VibeAround restarted in between. An invalid or reused code fails with a chat message rather than attaching anything.

## Switching the host agent

`/switch host <agent>` (or `/switch <agent>`, optionally `<agent>+<profile>`) behaves differently depending on what changes:

- **Different agent** → a **new thread** is created with the target host and a fresh CLI session; the old thread stays open but loses the route. Conversation context does not carry across agent products.
- **Same agent, different profile** → the current thread is kept and the **same session is preserved**; the agent host restarts under the new profile and resumes where it was.
- To get back to an earlier agent's conversation, use `/session` + `/session --switch <id>` — session records survive on their threads even after the route moved on.

## Multi-agent turns and subagents

A thread can run a multi-agent turn: the host agent uses the `initialize_subagents` / `wait_for_subagents` MCP tools to spawn named subagents (parallel, collaboration, or brainstorming mode) inside the same workspace. Each subagent is a full agent process with its own CLI session, tracked on the thread, with completion reports collected back into the host's turn. Interrupted subagents are recovered when the thread's runtime is rebuilt.

## Timing and limit reference

The warm-pool soft limit and eviction eligibility threshold, plus actual timers such as heartbeat/watchdog, code TTLs, and share-link expiry, live in one authoritative table: [Timers and limits](../reference/timers-and-limits.md).

---

*Source anchors: `src/core/src/workspace/threads/runtime.rs` (agent lifecycle, activity, busy/subagent state), `src/core/src/workspace/manager.rs` + `manager_routes.rs` (warm-pool limits and LRU eviction), `src/core/src/channels/prompt/` (commands, auto-close), `src/core/src/workspace/handover.rs` (in-memory pickup codes), `src/core/src/channels/prompt/handler.rs` (host start and switching), `src/server/src/web_server/mcp/mod.rs` (subagent tools), `src/core/src/process/supervisor.rs` (tick, watchdog).*
*Last verified: v0.7.11*

<sub>[◀ How it works](overview.md) · [Documentation index](../README.md) · [Channel plugin system ▶](channel-plugin-system.md)</sub>
