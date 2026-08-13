# Codex CLI From Phone

The scenario: Codex CLI is halfway through a long refactor on your desk machine, and you have to leave for lunch, a meeting, or the commute. With VibeAround the session keeps running on the local host, and your phone becomes the place where you review results, answer the agent's questions, and keep the task moving.

VibeAround is independent software and is not affiliated with OpenAI. Codex and ChatGPT are OpenAI products. If you are comparing hosted Codex workflows, SSH, tunnels, and VibeAround's local-host approach, read [Codex remote comparison](codex-remote.md) after this walkthrough.

## The Walkthrough

Prerequisites: VibeAround installed with Codex enabled ([install and onboarding](../guides/install-and-onboarding.md)), and Codex CLI working from a plain terminal.

1. **Connect a messaging channel once.** Telegram is the fastest: create the bot, paste its token into the desktop channel screen (or `channels.telegram` in `settings.json` + `va channel sync`). Details in [connect channels](../guides/connect-channels.md).
2. **Start the work at the desk.** Either launch Codex in your own terminal through VibeAround (desktop **Launch** screen: agent + workspace + profile, or `va launch --profile codex`), or just message your bot directly — the first message spawns a hosted Codex session in the workspace.
3. **Hand the terminal session to your phone.** In the launched CLI, run the handover tool (`/vibearound handover`). You get a short code, valid for two minutes. In your bot chat:

   ```text
   /pickup K7PQ
   ```

   The chat attaches to the terminal session — same context, same workspace.
4. **Steer from the phone.** Permission requests arrive as tappable cards. Useful commands while away:

   ```text
   /status        what am I attached to? busy or idle?
   /session       list resumable sessions in this workspace
   /new           abandon course, start a fresh thread
   ```

5. **Review outputs visually** with [Live Preview](../guides/web-dashboard.md): paired owner links and code-gated Server/Markdown Shares can open on the phone. Server Shares forward authenticated GET/HEAD paths, including page data reads; writes, protocol upgrades, service workers, WebSockets, and HMR are unsupported. `/va/*`, owner pages, chat, and review controls are excluded.

## What Phones Are Good At

Mobile is for steering, approval, and review — not broad file inspection.

- Ask the agent for status, approve or reject a proposed direction.
- Request a focused test or build and read the summary.
- Review a preview link.
- Pause, archive, or hand the session back to the desktop.

## Security Notes

Treat the phone as a control surface for the local workspace. Protect browser pairing, messaging channel membership, and terminal access the same way you protect direct shell access — see the [security model](../architecture/security-model.md).

## Related Docs

- [Codex remote comparison](codex-remote.md)
- [Remote coding](remote-coding.md)
- [IM usage — full command reference](../guides/im-usage.md)
- [Model profiles guide](../guides/model-profiles.md)

---

*Last verified: v0.7.11*

<sub>[Documentation index](../README.md)</sub>
