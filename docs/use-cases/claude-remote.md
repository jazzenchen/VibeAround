# Claude Code Remote Access

Claude remote access in VibeAround means reaching a local Claude Code session from another surface. The workspace stays on the host machine; VibeAround provides controlled ways to inspect, steer, and continue the session.

Claude and Claude Code are Anthropic products. VibeAround is independent software that coordinates local workflows.

Claude Code also has official Remote Control capabilities. VibeAround is useful when the workflow needs a shared local agent workspace, provider profiles, Web Terminal, messaging channels, live preview, or the same remote-control pattern across Claude Code, Codex CLI, Gemini CLI, OpenCode, and other agents.

## Typical Flow

1. Verify Claude Code works from a local terminal.
2. Add the repository as a VibeAround workspace.
3. Launch Claude Code from [Agent Launch](../guides/agent-launch.md).
4. Continue the session through [Session Handover](../architecture/session-lifecycle.md), [Web Terminal](../guides/im-usage.md), or a configured messaging channel.
5. Use [Live Preview](../guides/web-dashboard.md) for outputs that should be reviewed visually.

## Provider Switching

Some teams use Claude Code directly with its native configuration. Others use VibeAround provider profiles and API Bridge routes to keep model routing explicit. Choose the native path when it is already stable; choose profile launch when repeatable routing, aliases, or bridge translation matter.

See [Claude Code Provider Switcher](claude-code-switcher.md) for the provider-profile workflow.

## Operational Notes

- Keep one known-good local Claude Code setup before adding remote surfaces.
- Prefer private channels for first tests.
- Record which workspace a channel or handover link controls.
- Review tool actions before applying broad edits in important repositories.

## Related Docs

- [Agent Launch](../guides/agent-launch.md)
- [Provider Profiles & API Bridge](../guides/model-profiles.md)
- [Remote Coding](remote-coding.md)
- [Security model](../architecture/security-model.md)

---

*Mirrored to the website docs; originally authored there (2026-06), fact-check pass pending screenshots.*
*Last verified: v0.7.11*

<sub>[Documentation index](../README.md)</sub>
