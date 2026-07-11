# Channel plugin system

Every official IM integration — Slack, Discord, Telegram, Feishu, QQ Bot, WeCom, DingTalk, WhatsApp, and the Weixin/OpenClaw bridge — is a **channel plugin**: a separate Node.js process that speaks the platform's API on one side and a small ACP-based protocol to the VibeAround daemon on the other. This page explains how that system works. To configure an existing channel, see [Connect channels](../guides/connect-channels.md); to write a new plugin, see [Build a channel plugin](../guides/build-a-channel-plugin.md).

## Why out-of-process plugins

- **Isolation:** a platform SDK crash or memory leak kills one plugin process, not the daemon. The supervisor respawns it.
- **Ecosystem fit:** IM platform SDKs are overwhelmingly JavaScript; plugins run on Node.js with the `@vibearound/plugin-channel-sdk` npm package while the daemon stays Rust.
- **Independent shipping:** each plugin is its own repository and npm package, versioned and updated without a daemon release.

Two channels are built in rather than plugins: `web` (the dashboard's web chat) and `tui` run in-process over the same channel interfaces, which keeps one code path for message routing regardless of surface.

At server startup, both are registered as in-process channel instances in the same `ChannelManager`/`PluginHost` boundary. Their inbound events come from WebSocket/TUI adapters rather than a stdio child, then join the same `ConversationIngress` used by channel plugins.

## Where plugins live and how they are found

Plugins are discovered from the VibeAround data directory (`~/.vibearound/plugins/<id>/`), each with a manifest declaring `kind: "channel"`, its entry point, and configuration schema. The desktop onboarding flow installs plugin packages there; a project-local plugins directory is also scanned in development.

A discovered plugin only **runs** if its channel has configuration under `channels.<name>` in [`settings.json`](../reference/configuration.md#settingsjson) — no config means the plugin stays disabled.

## Process lifecycle

```text
register ──► spawn (node <entry>) ──► running ──► crash / freeze
                 ▲                                     │
                 └──── respawn after delay ◄───────────┘
```

The daemon's process supervisor owns every plugin process:

- **Crash respawn.** An exited plugin is respawned with bounded exponential backoff.
- **Heartbeat watchdog.** Plugins emit a `_va/heartbeat` notification every 30 seconds; if none arrives for 90 seconds the plugin is presumed frozen, killed, and respawned. This catches hung platform SDKs that never exit (values: [timers and limits](../reference/timers-and-limits.md#supervision)).
- **Live-only output.** IM output uses a small bounded in-memory transport buffer. It is never persisted or replayed after a plugin or daemon restart; a disconnected delivery is dropped and logged.
- **Abort-safe runtime and permission drain.** A generation-scoped cleanup guard removes only its own runtime and cancels pending permission waiters on normal exit, cancellation, panic, or supervisor task abort. A dead plugin therefore cannot leave a stale sender or hang an agent turn.
- **Platform health lease.** The SDK emits heartbeats only while the plugin's `healthCheck` succeeds. All official plugins implement a platform-aware check; real disconnect/auth-revoke fault injection and a typed `Starting/Ready/Degraded` status remain follow-up work.

You can manage the lifecycle manually: `va channels` (list), `va channel start|stop|restart <instance_id>`, `va channel sync` (reconcile running plugins against `settings.json`), or the equivalent desktop UI controls. Legacy single-instance settings currently use the channel kind as the instance id.

## The wire protocol, briefly

Plugin ↔ daemon communication is JSON-RPC over stdio using ACP framing. The important message shapes:

**Inbound (plugin → daemon):** a channel envelope — route key (channel kind, stable channel instance id, actor id, chat id, optional topic id), message id, sender, text, attachments — or a callback (button tap with an action value), or control inputs (stop, close).

**Outbound (daemon → plugin):** agent output chunks, lifecycle/session notices, system texts, turn status (for typing indicators), prompt-done markers, and **permission requests** carrying a request id plus a payload the plugin renders as platform-native interactive cards (Feishu cards use the V2 schema; Slack uses block actions, and so on). Every forwarded extension output carries the full route target; reply-bearing turn output — including startup lifecycle/session notices for that turn — additionally carries optional `replyTo`, the inbound platform message id. The SDK keeps streaming/rendering state isolated by that complete target. The plugin answers a permission request by sending the user's choice back with the same request id.

Attachments flow by reference: the plugin downloads platform files into the shared cache directory and passes safe file keys; the daemon turns them into resource links for the agent.

## Identity and routing

`channel_kind` selects the plugin implementation; `channel_instance_id` is the stable host-owned lifecycle/runtime key; `actor_id` identifies the addressed bot/actor on the platform. The complete durable route also includes `chat_id` and optional `topic_id`, so addressed actors and topics can attach to distinct workspace threads. `replyTo` is deliberately ephemeral: it controls where one turn is rendered but never selects or persists a workspace thread. Message ordering is FIFO per complete route, not global.

The host can now keep runtime state for distinct instance ids, but configuration and UI still expose one configured instance per channel kind. True same-kind multi-instance operation therefore remains a configuration/product task rather than a transport or renderer limitation.

## Relationship to the plugin repositories

The main repository contains the plugin *host* (discovery, supervision, transport). The plugins themselves — and the `@vibearound/plugin-channel-sdk` package they build on — live in separate repositories, each with a README covering platform-side setup (bot registration, permissions, webhooks). The wiki-level contract: this documentation covers the mechanism; per-platform setup steps live with each plugin.

---

*Source anchors: `src/core/src/plugins/` (discovery, manifest), `src/core/src/channels/` (transport_stdio, plugin_host, monitor), `src/core/src/process/supervisor.rs` (respawn, watchdog), `src/core/src/routing.rs` (RouteKey).*
*Last verified: `codex/im-acp-route-refactor` (2026-07-11).*

<sub>[◀ Session lifecycle](session-lifecycle.md) · [Documentation index](../README.md) · [Local API and bridge ▶](local-api-and-bridge.md)</sub>
