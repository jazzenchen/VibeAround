# Remote Coding With Local AI Agents

Remote coding in VibeAround means remote access to a local workspace. The agent, repository, terminal, credentials, package cache, and dev servers remain on the host machine. Browser, mobile, Web Terminal, messaging, and preview surfaces become controlled entry points back to that machine.

This is different from moving work into a hosted cloud IDE. VibeAround is for developers who want remote control, review, approvals, terminal reach, or mobile continuation while the trusted machine remains the place where commands run and project state lives.

## When To Use This

Use this workflow when:

- A coding agent is already running on a trusted laptop, desktop, workstation, or server.
- The project depends on local credentials, local services, private networks, hardware, or desktop tools.
- You need to step away from the keyboard but still review, approve, or redirect the session.
- You want remote reach without cloning the repository into a hosted cloud workspace.

## Recommended Setup

1. Install VibeAround from [Download](https://vibearound.com/docs/download).
2. Verify one agent works locally with [Quick Start](../guides/quick-tour.md).
3. Add the project as a workspace.
4. Start a session through [Agent Launch](../guides/agent-launch.md).
5. Enable one remote surface: [Session Handover](../architecture/session-lifecycle.md), [Remote Messaging & Web Terminal](../guides/im-usage.md), or [Live Preview](../guides/web-dashboard.md).
6. Review the [Security model](../architecture/security-model.md) before enabling tunnels or public-facing links.

## Remote Surfaces

| Surface | Best for |
| --- | --- |
| Web Chat | Short steering prompts and session continuation from a browser. |
| Web Terminal | Shell-like access to the local workspace. |
| Mobile browser | Quick review, approval, or redirection away from the desk. |
| Messaging channels | Asynchronous check-ins through Telegram, Feishu/Lark, Discord, Slack, WeChat, DingTalk, WeCom, or QQ Bot. |
| Live Preview | Reviewing local dev servers, Markdown, HTML, and generated artifacts. |

## Safety Checklist

- Confirm who can reach the session.
- Keep tunnels disabled until remote access is needed.
- Treat Web Terminal and messaging bots as privileged control surfaces.
- Use scoped preview links instead of broad access.
- Stop or archive sessions that should no longer accept input.

## Related Docs

- [Session Handover](../architecture/session-lifecycle.md)
- [Remote Messaging & Web Terminal](../guides/im-usage.md)
- [Security model](../architecture/security-model.md)
- [Codex Mobile Workflow](codex-mobile.md)

---

*Last verified: v0.7.11*

<sub>[Documentation index](../README.md)</sub>
