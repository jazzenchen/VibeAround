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
| `RouteKey` / `ChannelTarget` / `ActiveTurnTarget` | `routing.rs` | Durable conversation identity, ephemeral per-message delivery identity, and cancellation-safe current-turn origin |
| `PluginHost` | `plugin_host.rs` | Routing table: channel instance → live runtime; pending-permissions table |
| `PluginRuntime` | `plugin_runtime.rs` | Enum over stdio / websocket runtimes |
| `ChannelPluginRunner` / factory | `plugin_runner.rs` | Protocol owner for one supervised stdio plugin generation; rebuilt on every respawn |
| `ChannelMonitor` | `monitor.rs` | Dashboard facade over the supervisor for plugin lifecycle |
| `ChannelBridgeHandler` | `bridge_handler.rs` | Per-thread ACP client handler: notification fan-out + permission round-trip |
| Prompt handler | `prompt/handler.rs` | Business dispatch after the lane: command parse → thread ops → prompt |

## Interactions

- **← plugins/surfaces:** stdio plugins via the bridge; web/TUI via `WebChannelManager`'s registered senders.
- **→ workspace:** `prompt/handler.rs` calls `WorkspaceThreadManager` for route resolution, commands, prompts.
- **→ process:** the monitor registers plugin manifests with the `Supervisor`; the bridge factory re-registers the live runtime in `PluginHost` on every respawn.
- **← agent:** `ChannelBridgeHandler` receives ACP notifications/permission requests from hosted agents and turns them into outputs.

## Invariants — do not break

1. **Per-route ordering** belongs to `ConversationIngress`: the complete `RouteKey` selects one bounded lane. Web/TUI/stdio control paths must not bypass it.
2. **`handle_input` never waits on agent work** — it crosses an in-process async mailbox before route dispatch. This mailbox is not a durable product message queue and has no replay/attempt semantics.
3. **Every host/subagent-turn permission terminates**: the request is sent only to the active host target. Tap, active-turn cancellation, generation replacement, bridge death, and shutdown all complete it and drop its RAII registration; late responses are rejected. Add a new exit path for a plugin or prompt and it must preserve this invariant.
4. **IM output is live-only**: the stdio transport has a bounded in-memory buffer, but no durable queue. Disconnected delivery is dropped and never replayed after restart.
5. **Runtime ownership is instance-scoped**: heartbeat, output, permission cleanup, stop, and restart use `channel_instance_id`, while discovery and platform traits continue to use `channel_kind`.
6. **Current product scope is DM/Web:** direct messages need no mention. Group mention/callback parsing remains dormant in adapters and core, but is deferred from release acceptance.
7. `ChannelManager::shutdown_all` may stop only channel-owned supervised IDs; it must never drain the global supervisor.
8. **`replyTo` is ephemeral**: it may select a platform reply target and SDK renderer lane, but it must never enter `RouteKey`, persisted attachments, or workspace-thread selection.

## Known debt

- The upstream `ChannelManager` async mailbox remains unbounded even though route lanes and stdio plugin output are bounded. This is a capacity observation, not a proposal for a durable message queue; defer until measurements justify a cap.
- Web-chat session-intent side effects still run before route-lane serialization and can interleave across WebSocket connections.
- The route/target contract and SDK renderer now carry instance, actor, topic and per-message `replyTo`, but settings/UI still expose one configured instance per channel kind.
- `RouteKey::as_key()` remains a deliberately lossy legacy/display key and must not be reused as extended route identity.
- Runtime control lists and stops hosts by workspace thread id. Legacy `kind:chat` control keys are accepted only when they uniquely identify one live extended route.
- Host and subagent permissions are origin-scoped to the triggering host target and cancellation-safe.

---

*Source anchors: `src/core/src/channels/` (all files above), `src/server/src/lib.rs` (input dispatcher and ingress-first shutdown).*
*Last verified: `codex/im-acp-route-refactor` at `4a27a1c0` (2026-07-12).*

<sub>[◀ Flow: PTY terminal](../flows/web-terminal.md) · [Documentation index](../../README.md) · [Module: workspace ▶](workspace.md)</sub>
