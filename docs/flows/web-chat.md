# Flow: web chat

How a message typed in the dashboard's Web Chat reaches an agent. The back half is identical to the [IM message flow](im-message.md) — this page covers what is different at the web edge: the socket protocol, session intents, and replay.

## Connection setup

Opening Web Chat establishes `/ws/chat` (token-authenticated). On connect the server:

1. registers the connection with the `WebChannelManager` under the route's chat id (multiple tabs on one thread = multiple connections, all receiving the same fan-out),
2. sends a `Config` event (enabled agents, default agent),
3. replays recent output for the route so a reopened tab shows the tail of the conversation.

→ `src/server/src/web_server/ws_chat.rs`, `src/core/src/channels/transport_websocket.rs`

The web channel is **in-process**: instead of a stdio plugin there is a `WebSocketPluginRuntime` registered in the same `PluginHost` table the stdio plugins use — one outbound routing mechanism for all surfaces.

## Inbound message shapes

The browser sends typed JSON, not bare text. The main ones:

| Type | Meaning |
|---|---|
| message (+ optional `session_intent`, `profile`, `session_mode`) | A prompt, possibly with launch selection attached |
| `stop` | Cancel the in-flight turn |
| `PermissionResponse` | A tapped permission card ([permission flow](permission.md)) |
| `SetMode` / `SetConfigOption` | Change agent session mode / config option |
| `ResumeSession` | Attach a native CLI session to this web thread |

## The session-intent step

This is the web-specific part. Before dispatching the prompt, the socket handler applies any launch selection carried on the message:

- **`New { cwd }`** — create a fresh thread, in the given directory's workspace (or the current one).
- **`Resume { agent, session_id, cwd }`** — bind an existing native CLI session into the web thread (same mechanism as handover pickup).
- **none** — apply agent/profile selection to the route's current thread if it changed.

Then the message is enqueued as a normal `ChannelInput::Message` into the same sharded queue every channel uses, and from there the [IM message flow](im-message.md) steps 4–10 apply unchanged — same command grammar, same thread resolution, same agent path.

→ `ws_chat.rs` (`WebChatSessionIntent`, `apply_web_launch_selection`), then `src/core/src/channels/prompt/`

> Ordering note: the intent side-effects run in the socket task, before the queue's per-route serialization. With a single tab this is invisible; two tabs racing launch selections on the same thread can interleave. Tracked as a known cleanup in the remediation plan.

## Outbound: fan-out and idle

Outputs for web routes are dispatched to every registered connection for that chat id; each becomes a JSON `ChatEvent` (message chunks, tool status, permission cards, `PromptDone`).

Web threads participate in idle management: activity bumps the route's idle deadline, and an expired deadline unloads the agent (thread stays open, replay + resume make reopening seamless). Closing the tab does not close the thread.

→ `ws_chat.rs` (`output_to_chat_event`), `transport_websocket.rs` (idle bookkeeping)

## TUI

The TUI chat registers as its own in-process channel kind (`tui`) over the same WebSocket plugin runtime mechanism and `/ws/chat` contract — everything on this page applies to it except the browser-specific replay UI.

---

*Source anchors: `src/server/src/web_server/ws_chat.rs` (socket loop, intents, events), `src/core/src/channels/transport_websocket.rs` (WebChannelManager, idle), `src/server/src/lib.rs` (web/tui channel registration, dispatch task).*
*Last verified: v0.7.11*
