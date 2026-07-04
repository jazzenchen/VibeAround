# Channel plugin system

Every IM integration — Telegram, Slack, Feishu, Discord, WeChat, DingTalk, WeCom, QQ Bot — is a **channel plugin**: a separate Node.js process that speaks the platform's API on one side and a small ACP-based protocol to the VibeAround daemon on the other. This page explains how that system works. To configure an existing channel, see [Connect channels](connect-channels.md); to write a new plugin, see [Build a channel plugin](build-a-channel-plugin.md).

## Why out-of-process plugins

- **Isolation:** a platform SDK crash or memory leak kills one plugin process, not the daemon. The supervisor respawns it.
- **Ecosystem fit:** IM platform SDKs are overwhelmingly JavaScript; plugins run on Node.js with the `@vibearound/plugin-channel-sdk` npm package while the daemon stays Rust.
- **Independent shipping:** each plugin is its own repository and npm package, versioned and updated without a daemon release.

Two channels are built in rather than plugins: `web` (the dashboard's web chat) and `tui` run in-process over the same channel interfaces, which keeps one code path for message routing regardless of surface.

## Where plugins live and how they are found

Plugins are discovered from the VibeAround data directory (`~/.vibearound/plugins/<id>/`), each with a manifest declaring `kind: "channel"`, its entry point, and configuration schema. The desktop onboarding flow installs plugin packages there; a project-local plugins directory is also scanned in development.

A discovered plugin only **runs** if its channel has configuration under `channels.<name>` in `settings.json` — no config means the plugin stays disabled.

## Process lifecycle

```text
register ──► spawn (node <entry>) ──► running ──► crash / freeze
                 ▲                                     │
                 └──── respawn after delay ◄───────────┘
```

The daemon's process supervisor owns every plugin process:

- **Crash respawn.** An exited plugin is respawned after a short delay, indefinitely.
- **Heartbeat watchdog.** Plugins emit a `_va/heartbeat` notification every 15 seconds; if none arrives for 90 seconds the plugin is presumed frozen, killed, and respawned. This catches hung platform SDKs that never exit.
- **Outbox replay.** Durable outputs (system messages, permission requests) are queued in an outbox; if a send fails because the plugin is down, they are re-delivered after the respawn, so a permission card is not lost to a plugin restart.
- **Pending-permission drain.** If a plugin dies while a permission request is waiting for a tap, the pending request is cancelled so the agent's turn fails fast instead of hanging forever.

You can manage the lifecycle manually: `va channels` (list), `va channel start|stop|restart <kind>`, `va channel sync` (reconcile running plugins against `settings.json`), or the equivalent desktop UI controls.

## The wire protocol, briefly

Plugin ↔ daemon communication is JSON-RPC over stdio using ACP framing. The important message shapes:

**Inbound (plugin → daemon):** a channel envelope — route key (channel kind, bot id, chat id), message id, sender, text, attachments — or a callback (button tap with an action value), or control inputs (stop, close).

**Outbound (daemon → plugin):** agent output chunks, system texts, turn status (for typing indicators), prompt-done markers, and **permission requests** carrying a request id plus a payload the plugin renders as platform-native interactive cards (Feishu cards use the V2 schema; Slack uses block actions, and so on). The plugin answers a permission request by sending the user's choice back with the same request id.

Attachments flow by reference: the plugin downloads platform files into the shared cache directory and passes safe file keys; the daemon turns them into resource links for the agent.

## Identity and routing

Each plugin process represents one bot identity on one platform. The route key `(channel_kind, bot_id, chat_id)` isolates conversations per chat — group chats and DMs get independent threads, and two different chats never share agent state. Message ordering is guaranteed per route, not globally, so one busy group cannot stall another.

## Relationship to the plugin repositories

The main repository contains the plugin *host* (discovery, supervision, transport). The plugins themselves — and the `@vibearound/plugin-channel-sdk` package they build on — live in separate repositories, each with a README covering platform-side setup (bot registration, permissions, webhooks). The wiki-level contract: this documentation covers the mechanism; per-platform setup steps live with each plugin.

---

*Source anchors: `src/core/src/plugins/` (discovery, manifest), `src/core/src/channels/` (transport_stdio, plugin_host, outbox, monitor), `src/core/src/process/supervisor.rs` (respawn, watchdog), `src/core/src/routing.rs` (RouteKey).*
*Last verified: v0.7.11*
