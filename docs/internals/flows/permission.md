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

**2. Bridge handler captures the active origin and registers a oneshot.** The thread's `ChannelBridgeHandler` reads the current turn's `ChannelTarget`. With no active target it returns ACP **Cancelled** immediately. Otherwise it generates a fresh `request_id`, stores the origin channel instance plus a `oneshot::Sender` in `PluginHost::pending_permissions`, and emits one target-aware `ChannelOutput::PermissionRequest { request_id, payload }` only to that origin. There is deliberately **no human-response timeout** while a live card is waiting for a person.
→ `src/core/src/channels/bridge_handler.rs` (`request_permission`), `plugin_host.rs` (`pending_permissions`)

**3. Card renders.** The plugin turns the payload into a platform-native interactive card (Feishu V2 card, Slack block actions, Telegram inline keyboard). IM delivery is live-only: if the target runtime is absent or cannot accept the output, that surface is removed immediately and the waiter is cancelled when no eligible surface remains. On the web, the chat renders the card component and tracks it as pending.
→ plugin repos; `src/core/src/channels/plugin_host.rs`; `src/server/src/web_server/ws_chat.rs` (web cards)

**4. The tap comes back.** Two return paths into the same table:
- **Stdio plugins:** the tap arrives as an ACP response through the plugin bridge's forwarder, which pops `pending_permissions[request_id]` and fires the oneshot.
- **Web chat:** the browser sends a typed `PermissionResponse` over `/va/ws/chat`; the handler calls `respond_permission(channel_instance_id, request_id, response)`, which validates that the request belongs to that surface before firing.
→ `src/core/src/channels/transport_stdio/` (forwarder), `plugin_host.rs` (`respond_permission`)

**5. Agent resumes.** The bridge handler's `rx.await` completes with the chosen option and returns it as the ACP response. The agent continues (or aborts) the tool call accordingly.

## Why it can't hang forever

The no-timeout design needs cleanup guarantees instead:

| Situation | Guarantee |
|---|---|
| No live runtime accepts the card | That instance is removed immediately; when none remain, `rx.await` errors and the agent receives **Cancelled** |
| Plugin process dies or its bridge task is force-aborted while a card is pending | A generation-scoped Drop guard removes only that runtime and calls `cancel_channel_permissions(instance_id)`; the pending sender is dropped |
| User sends channel `/stop` while a card is pending | Dropping the blocked prompt drops its RAII registration, so the core entry is removed; the SDK also resolves the stale platform-side permission through its safest reject option and removes both callback indexes, so the next text cannot be swallowed as an answer to an old card |
| Daemon shutdown | `PluginHost::shutdown_all` clears the whole table first, same cancellation path |
| Tap for an already-resolved request | `respond_permission` finds nothing and returns "no longer pending" — the second tap is a no-op, not a double-approve |
| Tap from the wrong instance | Instance membership check leaves the entry untouched and rejects the response |

The net invariant for the host turn is: **every registered oneshot is consumed or dropped exactly once** — by the tap, prompt cancellation/drop, channel death, or daemon shutdown. An agent turn can wait indefinitely on a live human surface, but never on a dead process, and a different route attached to the same thread cannot approve the request.

> Remaining scope: subagent permission handling still uses its separate fan-out path. It is not governed by the host turn's `ActiveTurnTarget`, and its pending core registration does not yet have the same cancellation-safe RAII guard. Both origin selection and cleanup must be addressed when subagent session ownership is refactored.

---

*Source anchors: `src/core/src/channels/bridge_handler.rs` (request_permission), `src/core/src/channels/plugin_host.rs` (pending_permissions, respond_permission, cancel_channel_permissions, shutdown_all), `src/core/src/channels/transport_stdio/` (forwarder), `src/server/src/web_server/ws_chat.rs` (web response path).*
*Last verified: `codex/im-acp-route-refactor` at `ed12aa02` (2026-07-11).*

<sub>[◀ Flow: web chat](web-chat.md) · [Documentation index](../../README.md) · [Flow: bridge request ▶](bridge-request.md)</sub>
