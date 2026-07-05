# Concepts

VibeAround's runtime is built around six concepts: **workspace**, **thread**, **route**, **session**, **agent**, and **profile**. Every feature — IM chat, web chat, handover, agent switching — is a combination of these six. This page defines each one and how they relate.

## The model in one picture

```text
Route (telegram:chat_42) ──attached to──► Thread ──belongs to──► Workspace (~/dev/my-app)
Route (web:ws_wt_9f3e)   ──attached to──►   │
                                            │ hosts
                                            ▼
                                    Agent process (claude)
                                       │ launched with
                                       ▼
                                    Profile (moonshot)
                                       │ tracks
                                       ▼
                                    Session (CLI-native session id)
```

## Workspace

A workspace is a directory on your machine that agents work in — typically a project checkout. Workspaces are registered explicitly (desktop UI, `va workspace add`, or automatically when a thread needs one) and each gets a stable id. Two defaults exist for conversations that never chose a project: web chats fall back to the **General** workspace, and each IM channel gets `<default_workspace>/im/<channel>` on first contact — so a fresh Telegram chat works in `…/im/telegram` until you `/workspace --switch` it somewhere real.

- A workspace contains any number of threads.
- Deleting or switching a workspace never touches the directory contents; VibeAround only manages its own records.

*Details: [workspace module internals](../internals/modules/workspace.md) · [`va workspace` commands](../reference/cli.md#workspaces-previews-profiles) · [`default_workspace` / `workspaces` settings](../reference/configuration.md#settingsjson)*

## Thread

A thread is one conversation with continuity: the unit that owns "what we were talking about". Each thread lives in exactly one workspace and records which agent hosts it, which CLI sessions it has produced, and its open/closed status. Thread state is persisted as an event log, so closed threads remain inspectable and open threads survive a daemon restart.

- `/new` in a chat closes the current thread and starts a fresh one.
- `/close` closes the thread without starting a new one.
- A thread can host **subagents** — additional agent processes spawned for multi-agent turns — alongside its host agent.

*Details: [session lifecycle](session-lifecycle.md) (open/close rules, restart behavior) · [workspace module internals](../internals/modules/workspace.md) · [subagent MCP tools](../reference/api-surfaces.md#mcp-tools)*

## Route

A route is a stable address for one conversation path through a channel: the triple *(channel kind, bot id, chat id)* — for example `telegram : mybot : chat_42` or `web : ws_wt_9f3e`. Routes are how inbound messages find their thread: at any moment a route is attached to at most one open thread.

- Multiple routes can attach to the same thread. That is what session handover is: a second route attaching to the thread you started elsewhere.
- Messages on the same route are processed strictly in order; messages on different routes run in parallel.

*Details: [IM message flow](../internals/flows/im-message.md) (how a route resolves) · [channels module internals](../internals/modules/channels.md) · [handover flow](../internals/flows/handover.md)*

## Agent

An agent is a coding CLI that VibeAround can drive: Claude Code, Codex, Gemini CLI, Cursor, Qwen Code, Kiro, OpenCode, or Pi. VibeAround talks to agents over the [Agent Client Protocol](https://agentclientprotocol.com) (ACP) via each agent's ACP adapter, spawning one agent process per active thread. Desktop-only entries (`claude-desktop`, `codex-desktop`) are launch targets, not ACP runtimes — they open the vendor's desktop app instead.

- The agent hosting a thread can be changed with `/switch host <agent>`; the thread keeps its identity and the new agent takes over.
- Agent definitions (ACP adapter package, PTY command, resume template, config injection paths) come from a built-in registry.

*Details: [supported agents matrix](../product/supported-matrix.md#coding-agents) · [agent module internals](../internals/modules/agent.md) · [launch subsystem](../internals/launch.md) (how agents are started) · [`/switch` command reference](../guides/im-usage.md#agents-and-profiles)*

## Profile

A model profile is a saved provider configuration: which API endpoint, which credential, which models, and how the built-in bridge should translate between the agent's native API dialect and the provider's. Profiles let one provider subscription serve several different agent CLIs.

- A profile is chosen per launch or per thread host binding; the same thread can be re-hosted under a different profile.
- The special `direct` profile means "launch the agent with its own vendor login, no bridge involved".

*Details: [model profiles guide](../guides/model-profiles.md) (setup) · [provider endpoints reference](../reference/provider-endpoints.md) (plans, base URLs, models) · [bridge mechanism](local-api-and-bridge.md) · [profiles module internals](../internals/modules/profiles.md)*

## Session

A session is the agent CLI's own conversation record — the thing `claude --resume <id>` or `codex resume` restores. VibeAround observes session ids as agents create them and stores them on the thread, which is what makes cross-surface continuity work: pick up a terminal session in IM (`/pickup <code>`), resume a native session from the web dashboard, or hand a web session over to your phone.

- Sessions belong to the agent, not to VibeAround. VibeAround tracks and rediscovers them (including sessions created outside VibeAround) but the transcript lives in the agent's own storage.

*Details: [session lifecycle](session-lifecycle.md) (what survives what) · [handover flow](../internals/flows/handover.md) · [`va launch sessions` commands](../reference/cli.md#agents-and-launches)*

## How the pieces work together

A message arrives on a route. The route resolves to its attached open thread (creating a thread in a default workspace on first contact). The thread ensures its host agent process is running — launched in the workspace directory, with the bound profile's credentials materialized — and ensures an agent session exists. The prompt is forwarded over ACP; output streams back along every route attached to the thread. After ten idle minutes the agent process is shut down to save resources; the thread stays open and the next message transparently respawns the agent and resumes the session.

---

*Source anchors: `src/core/src/routing.rs` (RouteKey), `src/core/src/workspace/` (workspaces, threads, attachments), `src/core/src/resources.rs` + `src/resources/agents.json` (agent registry), `src/core/src/profiles/` (profiles), `src/core/src/workspace/manager.rs` (AGENT_HOST_IDLE_SHUTDOWN_DELAY).*
*Last verified: v0.7.11*

<sub>[◀ Troubleshooting and FAQ](../guides/troubleshooting-and-faq.md) · [Documentation index](../README.md) · [How it works ▶](overview.md)</sub>
