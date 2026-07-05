# Gemini CLI Remote Access

VibeAround can keep Gemini CLI sessions reachable while they continue running beside the local workspace. This is useful for developers who want Gemini CLI available from a phone or browser without relocating the project into a hosted environment.

Gemini is a Google product. VibeAround is independent software for coordinating local agent workflows.

## Setup Path

1. Install and authenticate Gemini CLI outside VibeAround.
2. Confirm it can access the intended workspace.
3. Add the workspace in VibeAround.
4. Start or continue the session through [Agent Launch](../guides/agent-launch.md).
5. Add handover, Web Terminal, messaging, or preview access after the local baseline works.

## Common Use Cases

- Ask Gemini CLI for a second opinion while another agent is editing.
- Continue investigation from a phone when the local host remains online.
- Use a provider profile when the workflow requires explicit endpoint or model selection.
- Review a generated Markdown or web preview through VibeAround's preview surfaces.

## Troubleshooting

If Gemini CLI does not work through VibeAround, first run it directly in the same workspace. Then check the selected terminal mode, profile route, environment variables, and local auth state.

## Related Docs

- [Supported AI Agents](../product/supported-matrix.md)
- [Provider Profiles & API Bridge](../guides/model-profiles.md)
- [Remote Messaging & Web Terminal](../guides/im-usage.md)
- [Troubleshooting](../guides/troubleshooting-and-faq.md)

---

*Last verified: v0.7.11*

<sub>[Documentation index](../README.md)</sub>
