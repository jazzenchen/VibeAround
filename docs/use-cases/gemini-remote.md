# Gemini CLI Remote Access

The scenario: Gemini CLI is investigating a bug or drafting changes on your desk machine, and you want to keep that session reachable from a phone or another browser — without relocating the project into a hosted environment. VibeAround keeps the session running beside the local workspace and gives you controlled remote doors back into it.

Gemini is a Google product. VibeAround is independent software for coordinating local agent workflows.

## The Walkthrough

1. **Confirm the local baseline.** Install and authenticate Gemini CLI outside VibeAround, and check it can work in the intended repository. A working local setup first makes every later step debuggable.
2. **Enable Gemini in VibeAround** during onboarding or from the desktop agent screen ([install and onboarding](../guides/install-and-onboarding.md)).
3. **Start or continue a session.** Desktop **Launch** screen (Gemini + workspace + profile) or `va launch --profile <name>` for your own terminal; or message a connected channel / Web Chat to spawn a hosted session ([agent launch guide](../guides/agent-launch.md)).
4. **Add the remote surface.** Connect a messaging channel ([connect channels](../guides/connect-channels.md)), then move a terminal session to it with `/vibearound handover` in the CLI and `/pickup <code>` in the chat — or just keep steering the hosted session from the same chat.
5. **Review outputs** through [Live Preview](../guides/web-dashboard.md): paired owner links and code-gated Server/Markdown Shares work from the phone. Server Shares show GET/HEAD pages and static assets only, not APIs, writes, workers, WebSockets, or HMR.

## Common Use Cases

- Ask Gemini CLI for a second opinion while another agent is editing — `/agent --switch <id>` moves a thread between enabled agents.
- Continue an investigation from a phone while the local host stays online.
- Use a provider profile when the workflow needs explicit endpoint or model selection ([model profiles guide](../guides/model-profiles.md)).
- Review a generated preview without pulling out a laptop.

## Troubleshooting

If Gemini CLI does not work through VibeAround, first run it directly in the same workspace. Then check the selected terminal mode, profile route, environment variables, and local auth state — the [troubleshooting guide](../guides/troubleshooting-and-faq.md) has the full checklist.

## Related Docs

- [Supported matrix](../product/supported-matrix.md)
- [Model profiles guide](../guides/model-profiles.md)
- [IM usage — full command reference](../guides/im-usage.md)
- [Remote coding](remote-coding.md)

---

*Last verified: v0.7.11*

<sub>[Documentation index](../README.md)</sub>
