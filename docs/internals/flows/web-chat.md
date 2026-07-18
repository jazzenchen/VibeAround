# Flow: web chat

How a message typed in the dashboard's Web Chat reaches an agent. The back half is identical to the [IM message flow](im-message.md) — this page covers what is different at the web edge: the socket protocol, session intents, and replay.

## Connection setup

Opening Web Chat establishes `/va/ws/chat` (token-authenticated). On connect the server:

1. registers the connection with the `WebChannelManager` under the complete route (multiple tabs on one thread = multiple connections, all receiving the same fan-out),
2. sends a `Config` event (enabled agents, default agent),
3. replays recent output for the route so a reopened tab shows the tail of the conversation.

→ `src/server/src/web_server/ws_chat.rs`, `src/core/src/channels/transport_websocket.rs`

The web channel is **in-process**: instead of a stdio plugin there is a `WebSocketPluginRuntime` registered in the same `PluginHost` table the stdio plugins use — one outbound routing mechanism for all surfaces. Web and TUI are therefore registered at the channel-hub/host boundary for routing, but they are not `ChannelPluginRunner` children and are not managed as channel plugin processes.

## Inbound message shapes

The browser sends typed JSON, not bare text. The main ones:

| Type | Meaning |
|---|---|
| message (+ optional `session_intent`, `profile`, `session_mode`) | A prompt, possibly with launch selection attached |
| `stop` | Cancel the active turn and invalidate older queued prompts on the route |
| `PermissionResponse` | A tapped permission card ([permission flow](permission.md)) |
| `SetMode` / `SetConfigOption` | Change agent session mode / config option |
| `ResumeSession` | Attach a native CLI session to this web thread |

`message` and `stop` first enter the same `ChannelManager` FIFO, so Stop cannot overtake a prompt that is still waiting upstream. Inside `ConversationIngress`, Stop becomes a route-lane control operation: it increments the lane's stop generation before calling runtime cancel, covering both a queued prompt and the race where a session exists but `agent.prompt` has not started yet.

## The session-intent step

This is the web-specific part. Before dispatching the prompt, the socket handler applies any launch selection carried on the message:

- **`New { cwd }`** — create a fresh thread, in the given directory's workspace (or the current one).
- **`Resume { agent, session_id, cwd }`** — bind an existing native CLI session into the web thread (same mechanism as handover pickup).
- **none** — apply agent/profile selection to the route's current thread if it changed.

Then the message is enqueued as a normal `ChannelInput::Message` into the same `ConversationIngress` every channel uses, and from there the [IM message flow](im-message.md) steps 4–10 apply unchanged — same route lane, command grammar, thread resolution and agent path.

→ `ws_chat.rs` (`WebChatSessionIntent`, `apply_web_launch_selection`), then `src/core/src/channels/prompt/`

> Ordering note: the intent side-effects run in the socket task, before the queue's per-route serialization. With a single tab this is invisible; two tabs racing launch selections on the same thread can interleave. Tracked as a known cleanup in the remediation plan.

## Outbound: fan-out and host residency

Outputs for web routes are dispatched to every connection registered for that complete route; each becomes a JSON `ChatEvent` (message chunks, tool status, permission cards, and `TurnStatus`). An inactive turn status is emitted after the turn's notification outputs and is the public completion boundary.

Web Chat has no route-specific process idle deadline. `TurnStatus { active: false }`, socket disconnect, and closing the tab do not unload the host or close the thread. Its host follows the same warm-thread pool policy as IM: it stays resident unless a later, genuinely new host puts the pool over its soft limit and this thread is the eligible least-recently-active candidate. Eviction retains the `ThreadRuntime` and session; reopening still gets output replay, and the next prompt resumes if needed.

→ `ws_chat.rs` (`output_to_chat_event`), `transport_websocket.rs` (connection fan-out), `workspace/manager_routes.rs` (shared warm-thread pool)

## TUI

The TUI chat registers as its own in-process channel kind (`tui`) over the same WebSocket plugin runtime mechanism and `/va/ws/chat` contract — everything on this page applies to it except the browser-specific replay UI.

## Verified smoke path

The 2026-07-11 refactor was exercised against a real standalone server and Codex ACP adapter:

- invalid token rejected with HTTP 401; authenticated non-upgrade request reached the WebSocket route and returned 400,
- two sockets on one route received the same `/help` system text and inactive turn status, then reconnect succeeded,
- a real Codex ACP turn produced `AgentReady`, `SessionReady`, streamed `WS_ACP_OK`, then emitted inactive turn status,
- Stop sent immediately after `SessionReady` produced inactive turn status and no agent text chunks,
- a same-socket Message followed immediately by Stop (without waiting for `SessionReady`) preserved FIFO order, produced inactive turn status, and emitted zero agent message chunks.
- a real two-turn Codex conversation reused one ACP session and recalled a token supplied only in the first turn.

---

*Source anchors: `src/server/src/web_server/ws_chat.rs` (socket loop, intents, events), `src/core/src/channels/transport_websocket.rs` (WebChannelManager, fan-out/replay), `src/server/src/lib.rs` (web/tui channel registration, dispatch task), `src/core/src/workspace/manager_routes.rs` (shared warm-thread pool).*
*Last verified: `codex/im-acp-route-refactor` at `4ef19537` (2026-07-11).*

<sub>[◀ Flow: IM message](im-message.md) · [Documentation index](../../README.md) · [Flow: permission request ▶](permission.md)</sub>
