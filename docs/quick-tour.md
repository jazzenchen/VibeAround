# Quick tour

Fifteen minutes, three "aha" moments: chat with a local agent in the browser, do the same from an IM app, then move one conversation between surfaces. Prerequisite: VibeAround installed with at least one agent enabled ([Install and onboarding](install-and-onboarding.md)).

## 1. First chat in the browser (3 minutes)

Open the dashboard (desktop app tray → Dashboard, or the URL from `va status`). Go to **Web Chat**.

Type something real:

> look at ~/dev/my-app and summarize what this project does

Watch what happens:

- An agent process spawns in a workspace — the first message creates the thread automatically, no setup.
- Output streams in as the agent reads files and works.
- If the agent wants to run a command beyond its defaults, a **permission card** appears; approve or deny inline.

Try `/status` in the chat to see what you are attached to: thread id, workspace, host agent, session.

## 2. Same agent, from your pocket (7 minutes)

Connect one IM channel — Telegram is the fastest first one:

1. Create a bot with [@BotFather](https://t.me/BotFather), copy the token.
2. Add the channel in the desktop app (or `settings.json` → `channels.telegram`, then `va channel sync`) — details in [Connect channels](connect-channels.md).
3. Open a chat with your bot and say hello.

Your bot chat is now a full agent conversation:

```text
/new                        start a fresh thread
fix the failing test in ~/dev/my-app and show me the diff
```

Permission requests arrive as tappable buttons. `/stop` cancels a runaway turn. This chat has its **own** thread — independent from your web chat, both can run simultaneously.

## 3. One conversation, two surfaces (5 minutes)

Continuity is the point, so move something:

**Terminal → phone.** Launch an agent CLI through VibeAround (desktop **Launch**, or `va launch --profile <name>`), work a bit, then ask the agent to run its handover tool (the `vibearound` skill exposes it as `/vibearound handover`). You get a short code. In your Telegram bot chat:

```text
/pickup 837261
```

The chat attaches to the terminal session — same context, same workspace, continue where you left off.

**Watch from two places.** Open Web Chat while an IM turn is running: threads fan output out to every attached surface, so you can watch the same turn stream in both.

## Where to go next

| You want to… | Read |
|---|---|
| Understand what just happened (threads, routes, sessions) | [Concepts](concepts.md) |
| Use one provider subscription across agent CLIs | [Model profiles guide](model-profiles-guide.md) |
| Reach the dashboard away from home | [Tunnels and remote access](tunnels-and-remote-access.md) |
| The full slash-command list | [IM usage](im-usage.md) |
| Share a running dev server for 10 minutes | [Web dashboard guide](web-dashboard-guide.md) |

---

*Source anchors: `src/core/src/channels/prompt/handler.rs` (commands), `src/core/src/workspace/handoff.rs` (pickup), `src/skills/vibearound/SKILL.md` (handover skill).*
*Last verified: v0.7.11*
