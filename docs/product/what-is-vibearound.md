# What is VibeAround

VibeAround turns the AI coding agents installed on your machine into something you can reach from anywhere: an IM chat on your phone, a browser tab, a desktop control app, or a terminal on another computer. The agents, the code, and the credentials never leave your machine — VibeAround adds the doors.

## The problem it solves

Coding agents like Claude Code and Codex are terminal programs. That is great while you sit at the keyboard, and useless the moment you step away. Real work does not stop when you leave the desk:

- The agent asks for permission to run a command five minutes after you left.
- A long refactor finishes and you want to review and reply from your phone.
- You started debugging in a terminal and want to continue from the sofa in a chat window.
- You want to preview a running dev server locally, or share rendered Markdown without deploying it.

VibeAround runs a local daemon that hosts your agents and exposes them through every surface you already use, with one consistent conversation model underneath.

## What you can do with it

**Chat with your local agent from any IM.** Connect Telegram, Slack, Feishu, Discord, WeChat, DingTalk, WeCom, or QQ Bot. The full coding flow works in chat: the agent writes code, runs commands, asks for permissions with tappable buttons, and streams results back — into a group or a private chat.

**Keep one conversation across surfaces.** Start in a terminal, run `/handover`, and type `/pickup <code>` in any connected IM to continue with full context. Web chats can be picked up on a phone the same way. Continuity works because VibeAround tracks the agent's own session ids, not chat transcripts.

**Use one model subscription across many CLIs.** Model profiles plus the built-in API bridge let a single provider account (Moonshot/Kimi, DeepSeek, OpenRouter, MiniMax, Z.AI/GLM, Gemini, Azure OpenAI, xAI, and others) power Claude Code, Codex, Gemini CLI, and any OpenAI-compatible client — including protocol translation between API dialects.

**Operate everything from a browser.** The web dashboard serves a real terminal (xterm over WebSocket), a web chat with permission cards, local-only dev-server previews, and rendered Markdown previews. The dashboard and Markdown owner previews can also be reached through a built-in tunnel (ngrok, localtunnel, Cloudflare, or Tailscale Funnel) after pairing; the current-key Markdown share link is the 10-minute, no-pairing exception.

**Launch agents your way.** The desktop app and `va launch` open agent CLIs in your own terminal with a saved profile: credentials materialized, model routing configured, project integrations installed.

**Run multiple agents at once.** Claude on Telegram and Codex on Slack simultaneously, each with independent threads and workspaces. Agents can also spawn parallel subagents for multi-agent turns inside one thread.

## What it is not

- **Not a cloud service.** There is no VibeAround server-side; the daemon, agents, credentials, and code stay on your machine. Tunnels are opt-in and pairing-protected.
- **Not another coding agent.** VibeAround does not compete with Claude Code or Codex; it hosts and routes the agents you already have, over the open Agent Client Protocol.
- **Not a chat proxy to a model API.** Conversations go through real agent CLIs with their own tools, context management, and session storage — the same agent you would run by hand, reachable from more places.

## The shape of the system

A Rust daemon (`vibearound-server`) owns the runtime: channel plugins, workspace threads, agent processes, the API bridge, previews, and tunnels. The desktop app (Tauri) embeds that daemon and adds GUI management. The web dashboard, the IM plugins, the TUI, and the `va` CLI are all clients of the same daemon — which is why a conversation can move between them without losing its place.

Continue with [Concepts](../architecture/concepts.md) for the six-term vocabulary the rest of the docs use, or [How it works](../architecture/overview.md) for the message flow.

---

*Source anchors: `README.md` (positioning), `src/server/src/lib.rs` (daemon composition), `src/resources/agents.json` (agent registry), `src/resources/profile-catalog/` (providers).*
*Last verified: v0.7.11*

<sub>[Documentation index](../README.md) · [Supported matrix ▶](supported-matrix.md)</sub>
