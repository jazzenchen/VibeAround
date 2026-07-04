# Flow: permission request

What happens between "the agent wants to run a command" and "you tapped Allow". This flow is the safety-critical one: its invariant is that a permission request always terminates — approved, denied, or cancelled — never silently dropped.

## Hop by hop

```text
agent ──ACP requestPermission──► bridge handler
                                     │ register oneshot (request_id)
                                     ▼
                              PermissionRequest output ──► plugin ──► card in chat
                                                                          │ tap
agent ◄──ACP response── bridge handler ◄──oneshot── forwarder ◄── callback with request_id
```

**1. Agent asks.** Mid-turn, the agent CLI sends ACP `session/request_permission` with the options (allow once, always, reject…). The agent's turn is now blocked on the reply.
→ `src/core/src/agent/runtime.rs` (client handler trait)

**2. Bridge handler registers a oneshot.** The thread's `ChannelBridgeHandler` generates a fresh `request_id`, stores `(channel_kind, oneshot::Sender)` in `PluginHost::pending_permissions`, and emits `ChannelOutput::PermissionRequest { request_id, payload }` to the routes attached to the thread. There is deliberately **no timeout** — the UX contract is "the user takes as long as they need".
→ `src/core/src/channels/bridge_handler.rs` (`request_permission`), `plugin_host.rs` (`pending_permissions`)

**3. Card renders.** The plugin turns the payload into a platform-native interactive card (Feishu V2 card, Slack block actions, Telegram inline keyboard). Permission requests are outbox-durable: if the plugin is down, the card is delivered after respawn. On the web, the chat renders the card component and tracks it as pending.
→ plugin repos; `src/core/src/channels/outbox.rs`; `src/server/src/web_server/ws_chat.rs` (web cards)

**4. The tap comes back.** Two return paths into the same table:
- **Stdio plugins:** the tap arrives as an ACP response through the plugin bridge's forwarder, which pops `pending_permissions[request_id]` and fires the oneshot.
- **Web chat:** the browser sends a typed `PermissionResponse` over `/ws/chat`; the handler calls `respond_permission(channel_kind, request_id, response)`, which validates that the request belongs to that channel before firing.
→ `src/core/src/channels/transport_stdio/` (forwarder), `plugin_host.rs` (`respond_permission`)

**5. Agent resumes.** The bridge handler's `rx.await` completes with the chosen option and returns it as the ACP response. The agent continues (or aborts) the tool call accordingly.

## Why it can't hang forever

The no-timeout design needs cleanup guarantees instead:

| Situation | Guarantee |
|---|---|
| Plugin process dies while a card is pending | The dying bridge calls `cancel_channel_permissions(kind)` exactly once — pending senders are dropped, `rx.await` errors, the handler answers the agent with **Cancelled** |
| Daemon shutdown | `PluginHost::shutdown_all` clears the whole table first, same cancellation path |
| Tap for an already-resolved request | `respond_permission` finds nothing and returns "no longer pending" — the second tap is a no-op, not a double-approve |
| Tap from the wrong channel | Channel-kind check re-inserts the entry and rejects the response |

The net invariant: **every registered oneshot is consumed exactly once** — by the tap, by channel cancellation, or by shutdown. An agent turn can wait indefinitely on a human, but never on a dead process.

---

*Source anchors: `src/core/src/channels/bridge_handler.rs` (request_permission), `src/core/src/channels/plugin_host.rs` (pending_permissions, respond_permission, cancel_channel_permissions, shutdown_all), `src/core/src/channels/transport_stdio/` (forwarder), `src/core/src/channels/outbox.rs` (durability), `src/server/src/web_server/ws_chat.rs` (web response path).*
*Last verified: v0.7.11*
