# Local AI Model Switcher

Local AI switch is the operating idea behind VibeAround's provider and agent control layer. Instead of scattering configuration across terminals, CLI config files, shell exports, and notes, VibeAround lets users choose the agent, workspace, session, profile, and route before work begins.

## What Can Be Switched

| Switch | Meaning |
| --- | --- |
| Agent | Claude Code, Codex CLI, Gemini CLI, OpenCode, Cursor CLI, Qwen Code, Kiro CLI, desktop agents, and related tools. |
| Workspace | The local folder or worktree where the agent should operate. |
| Session | A new session or a previous session to continue. |
| Provider profile | The model provider, endpoint, key reference, model name, and routing settings. |
| Bridge route | A local API Bridge path that adapts one API shape to another. |

## Why It Matters

Switching should be visible, reversible, and repeatable. A user should know which agent touched which workspace, which provider handled the request, and which session can be continued later.

## Setup Path

1. Finish [Quick Start](../guides/quick-tour.md) with one local agent.
2. Add one provider profile.
3. Launch the same agent with and without the profile.
4. Add a second agent only after the first route is stable.
5. Use [Unified Workspace](../architecture/concepts.md) to inspect session and profile state.

## Related Docs

- [Provider Profiles & API Bridge](../guides/model-profiles.md)
- [Agent Launch](../guides/agent-launch.md)
- [Supported Providers](../reference/provider-endpoints.md)
- [Supported AI Agents](../product/supported-matrix.md)

---

*Last verified: v0.7.11*

<sub>[Documentation index](../README.md)</sub>
