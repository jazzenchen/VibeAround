# Claude Code Provider Switcher

Provider switching is useful when a team wants Claude Code workflows to run through named profiles instead of repeatedly editing environment variables, endpoint URLs, or agent configuration. VibeAround's provider profiles and API Bridge make that routing visible at launch time.

## What A Switcher Should Solve

- Select a provider profile before starting a session.
- Keep model aliases and endpoint details out of ad hoc terminal notes.
- Route through a local bridge when the agent and provider use different API shapes.
- Preserve the ability to use Claude Code's native configuration when that is the best path.

## Recommended Workflow

1. Create one provider profile in [Provider Profiles & API Bridge](../guides/model-profiles.md).
2. Test the profile with a small task.
3. Add model aliases only after the base route works.
4. Launch Claude Code from VibeAround with the selected profile.
5. Record any provider-specific limitations in the profile notes.

## When Not To Switch

Do not add a profile layer when native Claude Code configuration is already the simplest, most reliable choice for the current task. Provider switching is most valuable when repeatability, explicit routing, model comparison, or bridge translation matters.

## Related Docs

- [Supported Providers](../reference/provider-endpoints.md)
- [Claude Remote Access](claude-remote.md)
- [Local AI Switch](local-ai-switch.md)
- [Architecture](../architecture/overview.md)

---

*Last verified: v0.7.11*

<sub>[Documentation index](../README.md)</sub>
