# OpenCode Remote Access

The scenario: OpenCode is running next to your repository and tools on the host machine, and you want to check on it from a phone during a break — or hand the session over entirely and continue from the sofa. VibeAround keeps execution on your own computer and provides browser, mobile, terminal, messaging, and preview entry points back to the session.

## The Walkthrough

1. **Confirm OpenCode runs from a local terminal** in the target repository. Fix auth and configuration there first.
2. **Enable OpenCode in VibeAround** ([install and onboarding](../guides/install-and-onboarding.md)) and add the project folder as a workspace.
3. **Launch or attach a session.** Desktop **Launch** screen or `va launch --profile <name>` for your own terminal; or message a connected channel / Web Chat for a hosted session ([agent launch guide](../guides/agent-launch.md)).
4. **Continue from another device.** In the CLI session run `/vibearound handover`, then `/pickup <code>` in your bot chat — or open Web Chat from a paired browser. Permission requests follow you as tappable cards.
5. **Add messaging only after the direct local session is stable** — it keeps every later problem attributable to one layer.

## Where It Fits

- Teams that want one workspace model around several coding agents — OpenCode beside Claude Code, Codex CLI, Gemini CLI, and others.
- Developers who want OpenCode reachable from a phone or a browser terminal.
- Workflows that need local dev servers, local package caches, or private network access.
- Sessions that benefit from preview links for generated web, Markdown, or HTML outputs ([Live Preview](../guides/web-dashboard.md)).

## Limitations To Verify

Support level can vary by OpenCode version, terminal mode, session persistence, and profile routing — the current state is tracked in the [supported matrix](../product/supported-matrix.md). Verify launch, continuation, and provider routing with a small task before using the workflow on important repositories.

## Related Docs

- [Supported matrix](../product/supported-matrix.md)
- [Remote coding](remote-coding.md)
- [IM usage — full command reference](../guides/im-usage.md)
- [Troubleshooting](../guides/troubleshooting-and-faq.md)

---

*Last verified: v0.7.11*

<sub>[Documentation index](../README.md)</sub>
