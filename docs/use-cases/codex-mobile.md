# Codex CLI From Phone

VibeAround can help users continue Codex CLI work from a phone while the actual session stays on the local host. This is useful when the agent needs a decision, a test result needs review, or a long task should keep moving while you are away from the desk.

Use this page when the goal is to control or inspect a local Codex CLI workflow from another device. If you are comparing hosted Codex workflows, official mobile access, SSH, tunnels, and VibeAround's approach, where execution stays on your own computer, read [Codex Remote Comparison](codex-remote.md) after this setup path.

VibeAround is independent software and is not affiliated with OpenAI. Codex and ChatGPT are OpenAI products.

## What This Workflow Provides

- A local host where Codex CLI can run beside the real repository.
- Browser and mobile entry points for session continuation.
- Web Terminal access when a shell view is needed.
- Messaging-channel access for short prompts and status checks.
- Optional provider profiles and API Bridge routes when a local workflow needs explicit model routing.

## Setup Path

1. Confirm Codex CLI works outside VibeAround.
2. Add the repository as a VibeAround workspace.
3. Launch or continue a Codex session from [Agent Launch](../guides/agent-launch.md).
4. Open the session from a mobile browser with [Session Handover](../architecture/session-lifecycle.md).
5. Add [Remote Messaging & Web Terminal](../guides/im-usage.md) only after the local session works.
6. Use [Live Preview](../guides/web-dashboard.md) to review local outputs from the same phone-friendly flow.

## Good Mobile Tasks

Mobile is best for steering, approval, and review. It is usually not the best place for broad file inspection.

- Ask the agent for status.
- Approve or reject a proposed direction.
- Request a focused test or build.
- Review a preview link.
- Pause, archive, or hand the session back to the desktop.

## Security Notes

Treat the phone as a control surface for the local workspace. Protect browser pairing, messaging channel membership, and terminal access the same way you protect direct shell access.

## Related Docs

- [Codex Remote Comparison](codex-remote.md)
- [Remote Coding](remote-coding.md)
- [Provider Profiles & API Bridge](../guides/model-profiles.md)
- [Security model](../architecture/security-model.md)

---

*Mirrored to the website docs; originally authored there (2026-06), fact-check pass pending screenshots.*
*Last verified: v0.7.11*

<sub>[Documentation index](../README.md)</sub>
