# Codex Remote Comparison

The scenario: you want Codex working while you are not at the machine, and you are weighing the options — hosted cloud environments, an SSH box, or remote control of your own computer. VibeAround targets the last pattern: Codex CLI keeps running beside your real repository, and VibeAround provides the remote doors back into it.

VibeAround is independent software and does not replace OpenAI's official Codex products.

## Positioning

| Workflow | Execution boundary | Best for |
| --- | --- | --- |
| Local Codex CLI with VibeAround | User-controlled host machine | Work that depends on local files, credentials, dev servers, tools, or multi-agent workflows. |
| Hosted cloud workspace | Provider-managed environment | Isolated tasks that can run in a configured remote environment. |
| SSH host | User-managed remote machine | Terminal-centric remote development with manual setup. |
| Mobile review surface | Remote control surface | Approvals, steering, status checks, and handover while work stays on a host. |

## What The Local-Host Pattern Looks Like

The concrete setup is a ten-minute path, walked through step by step in [Codex CLI from phone](codex-mobile.md):

1. Install VibeAround and confirm Codex CLI works locally.
2. Connect one messaging channel (Telegram is the fastest first one).
3. Launch Codex through VibeAround (`va launch --profile codex` or the desktop Launch screen), or message the bot to start a hosted session.
4. Hand a terminal session to the phone with the handover tool + `/pickup <code>`; approve permission cards from chat.

## Where VibeAround Helps

- You want Codex CLI beside other agents such as Claude Code, Gemini CLI, OpenCode, Cursor CLI, Qwen Code, or Kiro CLI — same workspaces, same remote surfaces.
- You want a browser workspace, Web Terminal, messaging channels, and previews around the same local session.
- You want provider profiles or API Bridge routes — for example running Codex against a third-party provider key instead of a subscription ([model profiles guide](../guides/model-profiles.md)).
- You do not want every task cloned into a cloud container.

## Practical Guidance

Use the official cloud or remote product when it fits the job directly. Use VibeAround when the important part of the workflow is your own machine: local workspace state, local services, private tools, multiple agents, provider switching, and remote entry points you control.

## Related Docs

- [Codex CLI from phone](codex-mobile.md)
- [Remote coding](remote-coding.md)
- [Supported matrix](../product/supported-matrix.md)
- [Architecture overview](../architecture/overview.md)

---

*Last verified: v0.7.11*

<sub>[Documentation index](../README.md)</sub>
