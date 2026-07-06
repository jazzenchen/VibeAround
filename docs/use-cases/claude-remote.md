# Claude Code Remote Access

The scenario: Claude Code is deep in a task on your desk machine and you are about to walk away — but the session should not stop, and you still want to answer its permission prompts and read its results. Claude remote access in VibeAround means the workspace stays on the host machine, and another surface — phone, browser, chat — becomes a controlled way to inspect, steer, and continue the same session.

Claude and Claude Code are Anthropic products. VibeAround is independent software that coordinates local workflows.

Claude Code also has official Remote Control capabilities. VibeAround is useful when the workflow needs a shared local agent workspace, provider profiles, Web Terminal, messaging channels, live preview, or the same remote-control pattern across Claude Code, Codex CLI, Gemini CLI, OpenCode, and other agents.

## The Walkthrough

Prerequisites: VibeAround installed with Claude Code enabled ([install and onboarding](../guides/install-and-onboarding.md)), and Claude Code working from a plain terminal.

1. **Connect one messaging channel** — create a Telegram bot, paste its token into the desktop channel screen (or `channels.telegram` in `settings.json` + `va channel sync`). Details in [connect channels](../guides/connect-channels.md).
2. **Start Claude Code through VibeAround.** Desktop **Launch** screen (agent + workspace + model profile), or from the CLI:

   ```bash
   va launch --profile claude
   ```

   You get Claude Code's full native TUI in your own terminal — VibeAround just rendered the environment ([agent launch guide](../guides/agent-launch.md)).
3. **Hand the session to your phone when you leave.** In the Claude Code session, run `/vibearound handover` — it prints a short code valid for two minutes. In your bot chat, type `/pickup <code>`. The chat attaches to the same session: same context, same workspace.
4. **Steer from chat.** Permission requests arrive as tappable cards; `/status` shows what you are attached to; `/new` starts fresh in the same workspace. Full command list in [IM usage](../guides/im-usage.md).
5. **Come back to the desk.** Launched sessions stay discoverable — `va launch sessions` lists them, and the dashboard resume pickers offer them for continuation.

## Provider Switching

Some teams use Claude Code directly with its native Anthropic login. Others use VibeAround provider profiles and API Bridge routes to run Claude Code against a third-party provider key — no coding-plan subscription required. Choose the native path when it is already stable; choose profile launch when repeatable routing, aliases, or bridge translation matter. See [Claude Code provider switcher](claude-code-switcher.md) for the concrete steps.

## Operational Notes

- Keep one known-good local Claude Code setup before adding remote surfaces.
- Prefer private channels for first tests.
- Record which workspace a channel or handover link controls — `/status` tells you.
- Review tool actions before applying broad edits in important repositories.

## Related Docs

- [Agent launch guide](../guides/agent-launch.md)
- [Claude Code provider switcher](claude-code-switcher.md)
- [Remote coding](remote-coding.md)
- [Security model](../architecture/security-model.md)

---

*Last verified: v0.7.11*

<sub>[Documentation index](../README.md)</sub>
