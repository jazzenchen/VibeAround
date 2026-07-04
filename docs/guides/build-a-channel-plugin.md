# Build a channel plugin

A channel plugin connects one IM platform to VibeAround: it turns platform events into channel envelopes and channel outputs into platform messages. This page gets you from zero to a running plugin; the architecture it plugs into is described in [Channel plugin system](../architecture/channel-plugin-system.md).

## What you are building

A Node.js process with three responsibilities:

1. **Listen** to your platform (webhook, long-poll, or WebSocket — your choice, the daemon does not care).
2. **Translate inbound**: platform message → channel envelope (route key, message id, sender, text, attachments) → send to the daemon.
3. **Translate outbound**: daemon outputs (agent text, system messages, turn status, permission requests) → platform messages/cards; button taps → callbacks with the request id.

The daemon owns everything else: routing, threads, agents, ordering, persistence, and your process's lifecycle (spawn, crash-respawn, heartbeat watchdog).

## Start from the SDK

```bash
npm i @vibearound/plugin-channel-sdk
```

The SDK wraps the stdio ACP transport: connection handshake, envelope/output types, heartbeat emission, and helpers for permission-card round-trips. Look at an existing plugin ([va-plugin-channel-telegram](https://github.com/jazzenchen/va-plugin-channel-telegram) is the smallest realistic reference; all ten are linked in the [supported matrix](../product/supported-matrix.md#im-channels)) and the [SDK repository](https://github.com/jazzenchen/va-plugin-channel-sdk) (published as `@vibearound/plugin-channel-sdk`) for the full message shapes.

### The wire shapes you will actually handle

Both directions are JSON with a `kind` tag, camelCase fields, and snake_case inside `route`/`attachments`. An inbound user message (plugin → daemon):

```json
{
  "kind": "message",
  "route": { "channel_kind": "telegram", "bot_id": "my_bot", "chat_id": "chat_42" },
  "messageId": "msg_1001",
  "text": "fix the failing test",
  "senderId": "user_7",
  "attachments": [
    { "message_id": "msg_1001", "file_key": "upload_ab12", "file_name": "log.txt", "resource_type": "text/plain", "size": 5120 }
  ]
}
```

A button tap comes back as a callback (empty `text`, the choice in `actionValue`):

```json
{ "kind": "callback", "route": { … }, "messageId": "msg_1002", "text": "", "actionValue": "allow_once" }
```

The permission request you must render as a card (daemon → plugin) — answer it through the SDK's `client.requestPermission` handler with the same `requestId`:

```json
{
  "kind": "permissionRequest",
  "route": { … },
  "requestId": "perm_9f3e",
  "payload": { "…": "a JSON-serialized ACP RequestPermissionRequest: tool call info + options" }
}
```

Other outputs you render or ignore: `threadReply` (streamed agent output), `systemText`, `turnStatus` (typing indicator on/off), `promptDone`, `sessionInfo` / `sessionMode` / `commandMenu` (richer UI), `multiAgentTurn` / `subagentStatus` (multi-agent progress). Control inputs you can send: `stop`, `close`, `log`.

> Version rule: depend on a published SDK version (`^x.y.z`). Never ship a plugin pinned to a local `file:` path.

## Manifest and layout

A plugin is a directory under `~/.vibearound/plugins/<kind>/` with a manifest declaring:

- `kind: "channel"` and the channel id (your route key's `channel_kind`),
- the Node entry point the daemon spawns,
- the configuration schema — what users must put under `channels.<your-kind>` in `settings.json`.

The daemon passes the user's `channels.<kind>` object to your process verbatim; validate it on startup and exit with a clear error if it is unusable (the supervisor surfaces crash loops to the user).

## The contract your plugin must honor

- **Heartbeat.** Emit `_va/heartbeat` on the SDK's cadence (every 15 s). Miss it for 90 s and the daemon assumes you are frozen and respawns you — so do not block the event loop on platform calls.
- **Route keys are identity.** `(channel_kind, bot_id, chat_id)` must be stable per conversation: same chat → same key, different chats → different keys. Report your real bot id once you know it; group chats with multiple bots depend on it.
- **Permission requests must resolve.** Render them as interactive cards/buttons; send the user's choice back with the same `request_id`. If your platform cannot render buttons, degrade to a text prompt and parse the reply — never drop a request silently.
- **Callbacks carry the action value.** A button tap becomes a callback with the action's value; the daemon turns it into the user's answer (or a prompt when the envelope has no text).
- **Attachments by safe reference.** Download platform files into the shared cache and pass the file key; keys with path separators or `..` are rejected by the daemon.
- **Exit on fatal, run forever otherwise.** Unrecoverable config errors: log and exit (users see a clean failure). Transient platform errors: retry internally; your process is expected to be long-lived.

## Developing against a live daemon

1. Run the daemon (desktop app or `va serve`).
2. Put your work-in-progress plugin in the user plugins directory (or the project plugins dir during development) with its manifest.
3. Add minimal config under `channels.<your-kind>`, then `va channel sync`.
4. Iterate: `va channel restart <your-kind>` after changes; `va channels` shows status; daemon logs show your stderr, tagged with your channel kind.
5. Verify the checklist from [Connect channels](connect-channels.md#verifying-a-new-channel): message → reply, `/status`, permission card round-trip, attachment in both directions.

## Publish

Plugins ship as their own repositories/npm packages, installed into the plugins directory by the desktop plugin manager. Your README must cover the platform-side setup a user needs: bot creation, tokens/permissions, webhook configuration, and the platform's known limits (card schema versions, message size, rate limits — Feishu cards, for example, must use the V2 card schema).

---

*Source anchors: `src/core/src/plugins/` (manifest, discovery dirs), `src/core/src/channels/transport_stdio/` (wire protocol), `src/core/src/channels/types.rs` (envelope/output shapes), `src/core/src/channels/monitor.rs` + `process/supervisor.rs` (heartbeat, respawn), `src/core/src/routing.rs` (route keys, attachment key rules); SDK: `@vibearound/plugin-channel-sdk`.*
*Last verified: v0.7.11*

<sub>[◀ Tunnels and remote access](tunnels-and-remote-access.md) · [Documentation index](../README.md) · [Build from source ▶](build-from-source.md)</sub>
