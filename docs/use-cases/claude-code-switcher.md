# Claude Code Provider Switcher

The scenario: you want to run Claude Code, but through a provider and key you chose — DeepSeek, Kimi, GLM, OpenRouter, or a self-hosted compatible endpoint — without editing environment variables before every session and without an official coding-plan subscription. VibeAround's provider profiles and API Bridge make that routing a named, visible choice at launch time.

Claude and Claude Code are Anthropic products. VibeAround is independent software that coordinates local workflows.

## The Walkthrough

1. **Create a provider profile.** In the desktop app's model profile screen:
   - Pick a provider from the catalog (Kimi/Moonshot, DeepSeek, OpenRouter, Z.AI/GLM, DashScope, and more — endpoints and models prefilled), or **custom** for any compatible endpoint.
   - Pick the endpoint variant where the provider has several (global vs CN, pay-as-you-go vs coding plan) — the key must match the plan.
   - Paste the API key. It is stored locally and only ever sent to that provider by the daemon.
   - Choose which models the profile exposes and which is the default. For agents that validate model names, define alias model ids that map to the real upstream model.

   Full details in the [model profiles guide](../guides/model-profiles.md); `va profiles` lists profiles from the CLI.
2. **Launch Claude Code with the profile.** Desktop **Launch** screen: pick Claude Code + workspace + the new profile — or from the CLI:

   ```bash
   va launch --profile <name>            # launch with the saved launch profile
   va launch --profile <name> --dry-run  # print the rendered plan without launching
   ```

   The rendered config points Claude Code's model traffic at the local bridge (`127.0.0.1:12358`); the daemon translates to the provider. Keep the daemon running for the session's lifetime.
3. **Test with a small task** before relying on the route. If the agent says "model not found", use the profile's alias model id, not the upstream id.
4. **Switch mid-conversation when hosted.** In an IM or web chat thread, `/profile --list` shows the profiles and `/profile --switch <id>` re-binds the thread — useful for comparing providers on the same task.

## When Not To Switch

Do not add a profile layer when native Claude Code configuration (your Anthropic login) is already the simplest, most reliable choice. Provider switching is most valuable when repeatability, explicit routing, model comparison, third-party keys, or bridge translation matter.

## The Same Switch, Everywhere

Profiles are not Claude Code specific. The same profile can launch Codex CLI, Gemini CLI, OpenCode, and the other supported agents, and any OpenAI-compatible tool can point at the profile's local endpoint (shown in the profile UI) to share the same provider setup. Agent, workspace, session, and profile are all selected per launch — visible, reversible, repeatable.

## Related Docs

- [Model profiles guide](../guides/model-profiles.md)
- [Provider endpoints reference](../reference/provider-endpoints.md)
- [Claude Code remote access](claude-remote.md)
- [Local API and bridge](../architecture/local-api-and-bridge.md)

---

*Last verified: v0.7.11*

<sub>[Documentation index](../README.md)</sub>
