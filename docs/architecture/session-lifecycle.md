# Session lifecycle

This page answers the operational questions: when does a conversation start and end, what happens on restart, and what exactly moves when you hand a session over or switch agents. Vocabulary is defined in [Concepts](concepts.md).

## Thread lifecycle

A thread is born the first time a route needs one — the first message in a chat, or an explicit `/new` — and stays **open** until something closes it:

| Event | Effect |
|---|---|
| `/new` | Closes the current thread, creates a fresh one in the same workspace, re-attaches the route |
| `/close` | Closes the thread; the next message will create a new one |
| Unrecoverable agent error (e.g. authentication required) | Thread auto-closes with the reason sent to the chat |
| Daemon shutdown | Threads stay open — thread state is an on-disk event log |

Closed threads keep their history in the event log; they are never silently deleted.

## Agent process lifecycle within a thread

The agent process hosting a thread is deliberately more ephemeral than the thread itself:

```text
first prompt ──► spawn agent ──► create/resume CLI session ──► turn ──► idle
                                                                          │ 10 min
      next prompt ◄── respawn + resume session ◄── agent shut down ◄──────┘
```

- **Idle shutdown:** ten minutes after the last activity, the host agent process is stopped. This is invisible in the chat — the thread remains open and the CLI session id is retained.
- **Transparent resume:** the next prompt respawns the agent and resumes the recorded CLI session, so context carries across the gap.
- **Crash:** agent processes are not auto-respawned mid-turn (restart policy is deliberate: crashes surface as errors instead of silently retrying). The next prompt starts a fresh process and resumes the session.

## What survives a daemon restart

| Thing | Survives? | Notes |
|---|---|---|
| Open threads and their route attachments | Yes | Rebuilt from event logs at startup |
| CLI session ids observed per thread | Yes | Stored in thread events |
| Conversation context inside a session | Yes | Owned by the agent CLI's own storage; restored via resume |
| In-flight turn | No | A turn interrupted by restart is lost; the session resumes at its last completed state |
| Web chat scrollback in the browser | Partially | Startup replay re-sends recent output for web routes |

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

## Timing reference

All lifecycle timers (idle shutdown, heartbeat/watchdog, code TTLs, share-link expiry) live in one authoritative table: [Timers and limits](../reference/timers-and-limits.md).

---

*Source anchors: `src/core/src/workspace/threads/runtime.rs` (agent lifecycle, busy/failed), `src/core/src/workspace/manager.rs` (AGENT_HOST_IDLE_SHUTDOWN_DELAY, attachments), `src/core/src/channels/prompt/` (commands, auto-close), `src/core/src/workspace/handoff.rs` (in-memory pickup codes), `src/core/src/channels/prompt/handler.rs` (switch_host: new-thread vs preserve-session split), `src/server/src/web_server/mcp/mod.rs` (subagent tools), `src/core/src/process/supervisor.rs` (tick, watchdog).*
*Last verified: v0.7.11*

<sub>[◀ How it works](overview.md) · [Documentation index](../README.md) · [Channel plugin system ▶](channel-plugin-system.md)</sub>
