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

Pickup codes are one-shot and expire quickly; an invalid or reused code fails with a chat message rather than attaching anything.

## Switching the host agent

`/switch host <agent>` (or `/switch <agent>`, optionally `<agent>+<profile>`) changes which CLI hosts the thread:

- The current agent process is stopped; the thread's host binding is rewritten; the next prompt spawns the new agent.
- The new agent starts a **new CLI session** — session transcripts do not transfer between different agent products. VibeAround performs a context transfer step so the new agent is briefed on the conversation so far, but tool-level state (running processes, shell state) does not carry.
- Switching back later resumes the previous agent's own session where it left off, because the thread retains per-agent session records.

## Multi-agent turns and subagents

A thread can run a multi-agent turn: the host agent uses the `initialize_subagents` / `wait_for_subagents` MCP tools to spawn named subagents (parallel, collaboration, or brainstorming mode) inside the same workspace. Each subagent is a full agent process with its own CLI session, tracked on the thread, with completion reports collected back into the host's turn. Interrupted subagents are recovered when the thread's runtime is rebuilt.

## Timing reference

| Timer | Value | Effect |
|---|---|---|
| Host agent idle shutdown | 10 minutes | Agent process stopped, thread stays open |
| Channel plugin heartbeat | 15 s expected / 90 s watchdog | Plugin killed and respawned if frozen |
| Supervisor respawn tick | 5 s | Granularity of crash-restart scheduling |
| Preview share links | short TTL (minutes) | Share URLs expire; owner URLs persist |

---

*Source anchors: `src/core/src/workspace/threads/runtime.rs` (agent lifecycle, busy/failed), `src/core/src/workspace/manager.rs` (AGENT_HOST_IDLE_SHUTDOWN_DELAY, attachments), `src/core/src/channels/prompt/` (commands, auto-close), `src/core/src/workspace/handoff.rs` (pickup codes), `src/core/src/workspace/context_transfer.rs` (switch briefing), `src/server/src/web_server/mcp/mod.rs` (subagent tools), `src/core/src/process/supervisor.rs` (tick, watchdog).*
*Last verified: v0.7.11*
