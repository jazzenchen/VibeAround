# OpenCode Remote Access

OpenCode remote access in VibeAround is a local-first workflow. OpenCode runs near the repository and tools on the host machine, while VibeAround provides browser, mobile, terminal, messaging, and preview entry points back to the session.

## Setup Path

1. Confirm OpenCode runs from a local terminal.
2. Add the project folder as a VibeAround workspace.
3. Launch or attach the session through [Agent Launch](../guides/agent-launch.md).
4. Use [Session Handover](../architecture/session-lifecycle.md) to continue from another device.
5. Add a messaging channel only after the direct local session is stable.

## Where It Fits

- Teams that want one workspace around several coding agents.
- Developers who want OpenCode available from a phone or browser terminal.
- Workflows that need local dev servers, local package caches, or private network access.
- Sessions that benefit from preview links for generated web, Markdown, or HTML outputs.

## Limitations To Verify

Support level can vary by OpenCode version, terminal mode, session persistence, and profile routing. Verify launch, continuation, and provider routing with a small task before using the workflow on important repositories.

## Related Docs

- [Supported AI Agents](../product/supported-matrix.md)
- [Remote Coding](remote-coding.md)
- [Live Preview](../guides/web-dashboard.md)
- [Troubleshooting](../guides/troubleshooting-and-faq.md)

---

*Mirrored to the website docs; originally authored there (2026-06), fact-check pass pending screenshots.*
*Last verified: v0.7.11*

<sub>[Documentation index](../README.md)</sub>
