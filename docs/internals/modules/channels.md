# Module: channels

`src/core/src/channels/` — everything between "a message arrived from some surface" and "a thread runtime got a prompt", plus the reverse direction. Flows through it: [IM message](../flows/im-message.md), [web chat](../flows/web-chat.md), [permission](../flows/permission.md).

## Responsibility

Host channel plugins (out-of-process stdio and in-process websocket), normalize all inbound traffic into `ChannelInput`, dispatch it to the workspace-thread layer, and route every `ChannelOutput` back to the surface that should render it. It owns message *transport and routing* — never conversation state (that is `workspace`) and never process spawning (that is `process`).

## Key types

| Type | File | Role |
|---|---|---|
| `ChannelManager` | `mod.rs` | Daemon-lifetime facade: input queue, plugin registration, sync, shutdown |
| `ConversationIngress` | `prompt/ingress.rs` | Shared business entry for stdio/Web/TUI; bounded full-route FIFO lanes, Stop generations and shutdown barrier |
| `ChannelInput` / `ChannelOutput` / `ChannelEnvelope` | `types.rs` | The wire vocabulary every surface speaks |
| `PluginHost` | `plugin_host.rs` | Routing table: channel kind → live runtime; pending-permissions table |
| `PluginRuntime` | `plugin_runtime.rs` | Enum over stdio / websocket runtimes |
| `ChannelPluginRunner` / factory | `plugin_runner.rs` | Protocol owner for one supervised stdio plugin generation; rebuilt on every respawn |
| `ChannelMonitor` | `monitor.rs` | Dashboard facade over the supervisor for plugin lifecycle |
| `ChannelOutbox` | `outbox.rs` | Durable queue for replayable outputs (system texts, permission cards) |
| `ChannelBridgeHandler` | `bridge_handler.rs` | Per-thread ACP client handler: notification fan-out + permission round-trip |
| `ConversationIngress` | `prompt/` | The single dispatch entry: route lane → command parse → thread ops → prompt |

## Interactions

- **← plugins/surfaces:** stdio plugins via the bridge; web/TUI via `WebChannelManager`'s registered senders.
- **→ workspace:** `prompt/handler.rs` calls `WorkspaceThreadManager` for route resolution, commands, prompts.
- **→ process:** the monitor registers plugin manifests with the `Supervisor`; the bridge factory re-registers the live runtime in `PluginHost` on every respawn.
- **← agent:** `ChannelBridgeHandler` receives ACP notifications/permission requests from hosted agents and turns them into outputs.

## Invariants — do not break

1. **Per-route ordering** belongs to `ConversationIngress`: the complete `RouteKey` selects one bounded lane. Web/TUI/stdio control paths must not bypass it.
2. **`handle_input` never blocks** — it is a queue send; platform-facing code must never wait on agent work.
3. **Every pending permission terminates**: registered oneshots are consumed by the tap, by `cancel_channel_permissions` on bridge death (exactly once per death), or by `shutdown_all`. Add a new exit path for a plugin and you must drain there too.
4. **Replayable vs direct outputs**: only durable kinds go through the outbox (`should_replay_output`); streaming chunks are intentionally lossy across plugin restarts.
5. Outbound sends for one channel hold that channel's send lock so respawn-replay and live sends cannot interleave.
6. **Addressing is explicit in groups:** direct messages need no mention; group text must mention the current bot. Callbacks count as explicit interaction. Platform plugins extract that semantic, and core enforces the normalized policy again.
7. `ChannelManager::shutdown_all` may stop only channel-owned supervised IDs; it must never drain the global supervisor.

## Known debt

- The upstream `ChannelManager` input queue and stdio plugin output queue remain unbounded even though route lanes are bounded.
- Web-chat session-intent side effects still run before route-lane serialization and can interleave across WebSocket connections.
- The route/SDK contract has `bot_id`, `actor_id` and `topic_id`, but official plugins still need to emit the metadata before multi-bot/multi-actor group routing is end to end.
- `run_acp_plugin_bridge` still takes ten arguments; `ChannelPluginRunner` is the natural context object for the next cleanup.

---

*Source anchors: `src/core/src/channels/` (all files above), `src/server/src/lib.rs` (input dispatcher and ingress-first shutdown).*
*Last verified: `codex/im-acp-route-refactor` at `0ba7fa2e` (2026-07-11).*

<sub>[◀ Flow: PTY terminal](../flows/web-terminal.md) · [Documentation index](../../README.md) · [Module: workspace ▶](workspace.md)</sub>
