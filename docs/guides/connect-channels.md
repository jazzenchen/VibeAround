# Connect channels

Connecting an IM channel takes three steps everywhere: create the bot on the platform, put its credentials under `channels.<kind>` in `settings.json`, and let VibeAround start the plugin. This page covers the VibeAround side and the general pattern; platform-side specifics (bot registration screens, permission scopes, webhook URLs) live in each plugin's own README.

## The pattern

1. **Install the plugin.** Desktop onboarding installs channel plugins into `~/.vibearound/plugins/<kind>/`; the desktop plugin manager adds or updates them later. A plugin that exists but has no configuration stays disabled.

2. **Configure the channel.** Add a `channels.<kind>` object to `~/.vibearound/settings.json`. The keys are plugin-specific — VibeAround passes the object through to the plugin verbatim. Two representative examples:

```jsonc
{
  "channels": {
    "telegram": {
      "bot_token": "123456:ABC-DEF..."
    },
    "feishu": {
      "app_id": "cli_a1b2c3...",
      "app_secret": "..."
    }
  }
}
```

3. **Start it.** The daemon starts configured plugins at boot. After editing settings while running, reconcile with:

```bash
va channel sync        # start newly configured, stop removed, restart changed
```

or use the desktop channel controls. `va channels` shows runtime status for every plugin.

## Choosing defaults per channel

The `remote` section assigns a default agent and profile per channel kind, used when a chat's first message creates a thread:

```jsonc
{
  "remote": {
    "channels": {
      "telegram": { "agent_id": "claude", "profile_id": "moonshot" },
      "feishu":   { "agent_id": "codex" }
    }
  }
}
```

Users can still `/switch` per thread; these are just starting values.

## Supported channels

Every channel has a dedicated setup page with platform-side steps and a code-verified config block:

| Channel | Setup page |
|---|---|
| Telegram | [channels/telegram](channels/telegram.md) |
| Feishu / Lark | [channels/feishu](channels/feishu.md) — permission JSON, long-connection subscriptions |
| Slack | [channels/slack](channels/slack.md) — one-paste app manifest, Socket Mode |
| Discord | [channels/discord](channels/discord.md) |
| DingTalk | [channels/dingtalk](channels/dingtalk.md) |
| WeCom | [channels/wecom](channels/wecom.md) |
| QQ Bot | [channels/qqbot](channels/qqbot.md) |
| WeChat | [channels/wechat](channels/wechat.md) — QR login |
| WhatsApp | [channels/whatsapp](channels/whatsapp.md) — QR login |

Kind ids and repository links are also in the [supported matrix](../product/supported-matrix.md#im-channels). Each plugin README additionally documents:

- how to create the bot and obtain credentials,
- required platform permissions/scopes,
- webhook vs long-poll modes where the platform offers both,
- platform capabilities and limits (card layouts, attachment size, rate limits).

## Health and lifecycle

Plugins are supervised: crash means respawn after a short delay, and a plugin that stops emitting heartbeats for 90 seconds is killed and respawned. Practical implications:

- A misconfigured credential typically shows as a crash-respawn loop — check `va channels` and the daemon logs, fix the config, then `va channel restart <kind>`.
- Undelivered system messages and permission cards are queued and re-sent after a plugin restart, so a flapping plugin does not eat approvals.
- Stopping a channel (`va channel stop <kind>`) does not close its threads; conversations resume when the plugin starts again.

## Verifying a new channel

1. `va channels` — the plugin shows as running.
2. Message the bot: expect a reply from the default agent (first contact creates the thread).
3. `/status` in the chat — confirms thread, workspace, agent, profile.
4. Trigger a permission (ask the agent to run a shell command) — confirm the card renders and the tap resolves it.

---

*Source anchors: `src/core/src/config.rs` (channel_names, channel raw config, RemoteConfig), `src/core/src/plugins/` (plugin directories), `src/core/src/channels/` (sync, supervision, live runtime routing), `src/cli/src/args.rs` (channel commands).*
*Last verified: v0.7.11*

<sub>[◀ IM usage](im-usage.md) · [Documentation index](../README.md) · [Model profiles guide ▶](model-profiles.md)</sub>
