# Module: channels

`src/core/src/channels/` — everything between "a message arrived from some surface" and "a thread runtime got a prompt", plus the reverse direction. Flows through it: [IM message](../flows/im-message.md), [web chat](../flows/web-chat.md), [permission](../flows/permission.md).

## Responsibility

Host channel plugins (out-of-process stdio and in-process websocket), normalize all inbound traffic into `ChannelInput`, dispatch it to the workspace-thread layer, and route every `ChannelOutput` back to the surface that should render it. It owns message *transport and routing* — never conversation state (that is `workspace`) and never process spawning (that is `process`).

## Key types

| Type | File | Role |
|---|---|---|
| `ChannelManager` | `mod.rs` | Daemon-lifetime facade: input queue, plugin registration, sync, shutdown |
| `ChannelInput` / `ChannelOutput` / `ChannelEnvelope` | `types.rs` | The wire vocabulary every surface speaks |
| `PluginHost` | `plugin_host.rs` | Routing table: channel kind → live runtime; pending-permissions table |
| `PluginRuntime` | `plugin_runtime.rs` | Enum over stdio / websocket runtimes |
| `ChannelPluginBridge` | `plugin_bridge.rs` | ProcessBridge impl driving one stdio plugin's ACP connection |
| `ChannelMonitor` | `monitor.rs` | Dashboard facade over the supervisor for plugin lifecycle |
| `ChannelOutbox` | `outbox.rs` | Durable queue for replayable outputs (system texts, permission cards) |
| `ChannelBridgeHandler` | `bridge_handler.rs` | Per-thread ACP client handler: notification fan-out + permission round-trip |
| `handle_channel_input` | `prompt/` | The single dispatch entry: command parse → thread ops → prompt |

## Interactions

- **← plugins/surfaces:** stdio plugins via the bridge; web/TUI via `WebChannelManager`'s registered senders.
- **→ workspace:** `prompt/handler.rs` calls `WorkspaceThreadManager` for route resolution, commands, prompts.
- **→ process:** the monitor registers plugin manifests with the `Supervisor`; the bridge factory re-registers the live runtime in `PluginHost` on every respawn.
- **← agent:** `ChannelBridgeHandler` receives ACP notifications/permission requests from hosted agents and turns them into outputs.

## Invariants — do not break

1. **Per-route ordering** comes from the server's shard workers; nothing in this module may spawn per-message tasks that bypass it.
2. **`handle_input` never blocks** — it is a queue send; platform-facing code must never wait on agent work.
3. **Every pending permission terminates**: registered oneshots are consumed by the tap, by `cancel_channel_permissions` on bridge death (exactly once per death), or by `shutdown_all`. Add a new exit path for a plugin and you must drain there too.
4. **Replayable vs direct outputs**: only durable kinds go through the outbox (`should_replay_output`); streaming chunks are intentionally lossy across plugin restarts.
5. Outbound sends for one channel hold that channel's send lock so respawn-replay and live sends cannot interleave.

## Known debt

- `channel_kind == "web"` string special-cases (7 sites across core) should become declared channel traits — remediation M4.
- Web-chat session-intent side effects run before queue serialization — remediation M6.
- `send_locks` map entries are never pruned (bounded by channel count; cosmetic).

---

*Source anchors: `src/core/src/channels/` (all files above), `src/server/src/lib.rs` (shard workers).*
*Last verified: v0.7.11*

<sub>[◀ Flow: PTY terminal](../flows/web-terminal.md) · [Documentation index](../../README.md) · [Module: workspace ▶](workspace.md)</sub>
