# IM usage

How to drive a coding agent from a chat window, and the complete slash-command reference. This page applies to every channel — Telegram, Slack, Feishu, Discord, WeChat, DingTalk, WeCom, QQ Bot — plus the built-in web chat, which understands the same commands.

## The basics

Any message that is not a command is a prompt to the agent hosting your chat's thread. The first message in a chat creates a thread automatically in a default workspace. Output streams back as it is produced; long tool runs show progress rather than silence.

**Permission cards.** When the agent needs approval (run a command, edit outside its allowed scope), the chat receives an interactive card with the choices. The agent waits until you tap one. Cards survive plugin restarts — an unanswered card is re-delivered.

**Attachments.** Files you send are downloaded and handed to the agent as file references; the agent can read them from its workspace context. Images work with agents/models that accept image input.

**Group chats.** Each (bot × chat) pair is its own route with its own thread, so a group discussion and your DM with the same bot are separate conversations. Multiple bots in one group each keep their own thread.

## Command reference

Commands work with either prefix style: `/command`, or `/va command` / `va command` (useful on platforms that reserve slash commands). `/vibearound` is an alias for `/va`.

### Thread control

| Command | Effect |
|---|---|
| `/new` | Close the current thread and start a fresh one in the same workspace using the channel's configured agent/profile |
| `/close` | Close the thread; next message starts a new one |
| `/status` | Show thread id, workspace, host agent, profile, session, busy state |
| `/help` (or `/commands`, `/va`) | Show command help |

### Stopping work

Cancelling an in-flight turn is a channel-level signal, not a host slash command: the web chat has a stop button, and each IM plugin exposes its own affordance (a stop command or button — see the plugin's README). The signal is delivered even while the agent is busy.

### Workspaces

| Command | Effect |
|---|---|
| `/workspace` | List registered workspaces, marking the current one |
| `/workspace --switch <id-or-name>` | Move this chat to another workspace (starts a thread there) |
| `/switch workspace <token>` | Same as above, alternate spelling |

### Agents and profiles

| Command | Effect |
|---|---|
| `/agent` (or `/agent --list`) | List enabled agents |
| `/agent --switch <id>` | Switch this thread's host agent |
| `/switch <agent>` | Shorthand for a host switch, e.g. `/switch codex` |
| `/switch host <agent> [profile]` | Switch host with an explicit profile |
| `/switch <agent>+<profile>` | Same, compact form, e.g. `/switch claude+moonshot` |
| `/profile` (or `/profile --list`) | List model profiles |
| `/profile --switch <id>` | Re-bind the thread to another profile |
| `/agent <anything else>` | Pass through as a native command to the hosted agent (e.g. `/agent compact`) |

Switching to a different agent creates a new thread with a fresh session; switching only the profile keeps the current thread and session. See [Session lifecycle](../architecture/session-lifecycle.md).

### Sessions and continuity

| Command | Effect |
|---|---|
| `/session` (or `/session --list`) | List resumable sessions for this workspace/agent |
| `/session --switch <id>` | Attach this chat to an existing session (prefix match accepted) |
| `/pickup <code>` | Pick up a handover code issued from a terminal or web session |
| `/pair <code>` | Confirm a browser pairing code for tunneled dashboard access |

### Unknown commands

Anything starting with `/` that does not parse is reported back as unknown rather than being sent to the agent — typos will not accidentally become prompts.

## Practical patterns

- **Fire-and-walk-away:** kick off a long task, pocket the phone; the permission card pings you when the agent needs a decision, and the reply streams when it finishes.
- **Two agents in parallel:** `/switch codex` in one chat while another chat stays on Claude — different threads, independent turns.
- **Rescue a stuck turn:** stop it (stop button or the plugin's stop command), then `/status` to confirm idle, then re-prompt. If the thread itself is wedged, `/new` gives a clean start in the same workspace.
- **Continue last night's terminal session:** `/session` to find it, `/session --switch <id>` to attach — works for sessions created outside VibeAround too.

---

*Source anchors: `src/core/src/channels/prompt/handler.rs` (parse_thread_command — the command grammar above mirrors it 1:1), `src/core/src/channels/prompt/mod.rs` (attachments, callbacks), `src/core/src/channels/types.rs` (Stop/Close inputs).*
*Last verified: v0.7.11*

<sub>[◀ Web dashboard guide](web-dashboard.md) · [Documentation index](../README.md) · [Connect channels ▶](connect-channels.md)</sub>
