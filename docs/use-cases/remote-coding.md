# Remote Coding With Local AI Agents

The scenario: a coding agent is working on a repository on your desk machine — with your credentials, your dev servers, your private network — and you need to leave. Remote coding in VibeAround means the work stays exactly where it is, and your phone or another browser becomes a controlled door back into it.

This is different from moving work into a hosted cloud IDE. Nothing gets cloned into a provider-managed container; browser, mobile, messaging, and preview surfaces all lead back to the same local workspace.

## When To Use This

- A coding agent is already running on a trusted laptop, desktop, workstation, or server.
- The project depends on local credentials, local services, private networks, hardware, or desktop tools.
- You need to step away from the keyboard but still review, approve, or redirect the session.
- You want remote reach without cloning the repository into a hosted cloud workspace.

## The Walkthrough

Fifteen minutes if VibeAround is already installed; add ten for a first install.

1. **Install VibeAround** — desktop app from the [releases page](https://github.com/jazzenchen/VibeAround/releases), or `npm i -g @vibearound/cli` then `va serve` for headless setups ([install and onboarding](../guides/install-and-onboarding.md)).
2. **Verify the agent works locally.** Open the dashboard (tray → Dashboard, or the URL from `va status`), go to **Web Chat**, and give it a real task in your repository. The first message creates the thread automatically ([quick tour](../guides/quick-tour.md)).
3. **Connect one messaging channel.** Telegram is the fastest first one: create the bot on Telegram's side, then paste its token into the desktop channel screen — or add it under `channels.telegram` in `settings.json` and run `va channel sync` ([connect channels](../guides/connect-channels.md)). Your bot chat is now a full agent conversation with tappable permission cards.
4. **Move a terminal session to your pocket.** In an agent CLI launched through VibeAround, run the handover tool (`/vibearound handover`) — you get a short code, valid for two minutes. In the bot chat, type `/pickup <code>`. Same context, same workspace, continue where you left off.
5. **For browser access away from home**, enable a tunnel. The first visit from a remote browser shows a 6-digit pairing gate before anything is reachable ([tunnels and remote access](../guides/tunnels-and-remote-access.md)).

## Pick Your Remote Surface

| Surface | Best for |
| --- | --- |
| Web Chat | Short steering prompts and session continuation from a browser. |
| Mobile browser | Quick review, approval, or redirection away from the desk. |
| Messaging channels | Asynchronous check-ins through Telegram, Feishu/Lark, Discord, Slack, WeChat, DingTalk, WeCom, or QQ Bot. |
| Live Preview | Reviewing dev servers/HTML in the host's local browser and rendered Markdown remotely. |

## Safety Checklist

- Confirm who can reach the session — channel membership and browser pairing are the access boundary.
- Keep tunnels disabled until remote access is actually needed.
- Treat Web Chat and messaging bots as privileged control surfaces; protect them like shell access.
- Use scoped preview links instead of broad access.
- Stop or archive sessions that should no longer accept input.

Review the [security model](../architecture/security-model.md) before enabling tunnels or public-facing links.

## Related Docs

- [Session handover](../architecture/session-lifecycle.md)
- [IM usage](../guides/im-usage.md)
- [Codex on mobile](codex-mobile.md) · [Claude Code remote](claude-remote.md) · [Gemini CLI remote](gemini-remote.md) · [OpenCode remote](opencode-remote.md)

---

*Last verified: v0.7.11*

<sub>[Documentation index](../README.md)</sub>
