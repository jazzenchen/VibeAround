# Flow: IM message

One message in a Telegram/Feishu/Slack chat, followed from platform event to streamed reply. This is the trunk flow — [web chat](web-chat.md) and [permission](permission.md) branch off it. File references are repo-relative; line-level detail lives in the anchored files' module docs.

## Hop by hop

```text
platform ─1─► plugin ─2─► ChannelPluginRunner ─3─► ConversationIngress
                                                     │4 bounded RouteLane
                                  ┌──────────────────┘
                                  ▼
                         command? ──yes──► thread command 5
                            │no
                            ▼6                ▼7                    ▼8
                      resolve route ──► AcpSessionRunner ──► ACP prompt
                                                                  │
                 chat ◄─10─ plugin ◄─ ChannelOutput ◄─9─ notifications
```

**1. Platform → plugin.** The channel plugin (its own Node.js process) receives the webhook/long-poll event, applies platform semantics, downloads attachments into `~/.vibearound/.cache/`, and builds an SDK prompt. Direct messages may address the bot implicitly; group text must mention the current bot, while an explicit callback remains addressable without a text mention. The logical route is `(channel_kind, bot_id, chat_id, actor_id?, topic_id?)`.
→ plugin repo; envelope type in `src/core/src/channels/types.rs`

**2. Plugin → daemon.** The SDK sends stdio JSON-RPC/ACP. `ChannelPluginRunner` owns one protocol generation; the transport decodes `agent/prompt` and `va.channel` metadata into a `ChannelInput`. The maintained plugin branches use `sendChannelPrompt` rather than raw `agent.prompt`, including available sender/message/topic identity. Legacy third-party plugins without metadata still work, but default `bot_id/actor_id` values do **not** constitute multi-bot support.
→ `src/core/src/channels/plugin_runner.rs`, `transport_stdio/`, `types.rs`

**3. Enqueue.** `ChannelManager::handle_input` is fire-and-forget: the input goes onto an unbounded mpsc queue. Nothing platform-facing ever blocks on agent work.
→ `src/core/src/channels/mod.rs` (`handle_input`)

**4. Route lane.** `ConversationIngress` places prompt-bearing work into a bounded FIFO lane keyed by the complete `RouteKey` (capacity 16). Same route is serialized; different routes run independently without hash-collision head-of-line blocking. `Stop` increments the lane's stop generation, cancels the active runtime and discards older queued work. SDK stop aliases (`/stop`, `/cancel`, `va stop`) use ACP cancel metadata so the host cancels only the addressed actor/topic route; metadata-free plugins retain the old chat-wide fallback. Daemon shutdown closes ingress first and waits for all lane tasks to drain.
→ `src/core/src/channels/prompt/ingress.rs`

**5. Command parse.** The text is checked against the slash-command grammar (`/new`, `/close`, `/switch`, `/pickup`, `/status`, resource commands, `/va` prefix forms). The core address policy is a defense-in-depth check: group text must be `Mention`-addressed and a callback is an explicit interaction; bare group commands are rejected. Commands execute against the workspace-thread layer and answer with system texts.
→ `src/core/src/channels/prompt/handler.rs` (`parse_thread_command`, `handle_command`)

**6. Route → thread runtime.** `resolve_route_runtime` looks up the route's attachment: attached open thread → its runtime; no attachment → create a default workspace, persist a new thread event, attach the route. On the first extended-route message after upgrading an old plugin, a per-legacy-route migration lock lets the base route adopt and then detach `(kind, kind, chat)` so existing thread/session continuity is preserved. A distinct Slack-style topic may not steal the base attachment; a Discord thread whose `chatId == topicId` may adopt its own same-id legacy route. Because `bot_id`, `actor_id` and `topic_id` are part of the route, later addressed actors in one group can map to distinct threads. Maintained plugins now populate the contract, but the host registry is still keyed by one `channel_kind` and the SDK renderer is chat-keyed; multiple simultaneous Bot instances and topic-aware return rendering therefore remain staged rather than a shipped multi-bot guarantee.
→ `src/core/src/workspace/manager.rs` (`resolve_route_runtime`)

**7. Ensure agent + session.** `ThreadRuntime` keeps durable session identity separately from an `AcpSessionRunner`, which owns the live `Agent` and handler for one generation. A dead generation is replaced as a unit. Agent spawn is registered with the supervisor (`Never` restart policy), performs ACP initialize, then creates or resumes the recorded CLI session. IM route attachments are rehydratable but do not replay old output: idle host unload preserves the same thread/profile/session and the next message resumes it.
→ `src/core/src/workspace/threads/runtime.rs` (`ensure_agent`, `ensure_session`)

**8. Prompt.** Text + attachment resource links become ACP content blocks; `session/prompt` is sent. Route-lane ordering and the thread's prompt lock both apply: the first protects route intent, the second protects a shared thread reached from multiple routes.
→ `runtime.rs` (`prompt`), `src/core/src/channels/prompt/ingress.rs`

**9. Notifications → outputs.** Every ACP `session_notification` from the agent is wrapped as a thread reply and fanned out as `ChannelOutput` to **every route attached to the thread**. Output targets preserve the extended route fields; older SDK renderers currently consume only `chatId`.
→ `src/core/src/channels/bridge_handler.rs` (`session_notification`)

**10. Output → chat.** `PluginHost` routes each output to the owning plugin's live runtime. Durable kinds (system texts, permission requests) are staged in the outbox first and replayed if the plugin is down. The plugin renders platform-native messages.
→ `src/core/src/channels/plugin_host.rs` (`send_output`), `outbox.rs`

**Epilogue.** After the turn: `PromptDone` (typing indicator off), errors sent as `❌` system text (auth errors auto-close the thread), and a 10-minute idle shutdown scheduled for the host agent. The thread and its session id persist; the next message transparently respawns.
→ `src/core/src/channels/prompt/ingress.rs`, `manager.rs` (idle shutdown)

## Failure behavior along the path

| Failure | Result |
|---|---|
| Plugin crashes before step 2 | Platform may redeliver; supervisor records the real exit status and respawns with bounded exponential backoff |
| Daemon restarts between 4 and 8 | Ingress closes before thread/process teardown; the in-flight turn is cancelled, while thread + session persist |
| Agent spawn fails at 7 | `❌` system text; auth-required errors auto-close the thread |
| Agent crashes mid-turn at 8 | Turn errors out; next prompt spawns fresh and resumes the session |
| Plugin freezes after startup | Heartbeat watchdog performs a full generation restart; a bridge that ignores cancel is tree-killed and bounded-aborted before respawn |
| Plugin dead at 10 | Durable outputs wait in the outbox for the respawned plugin † |

> † Known gap: this holds only while no runtime is registered for the channel. In the crash window the dead runtime is still routable, so durable outputs are marked sent (queued into the dead bridge) or nacked-and-dropped instead of waiting. Tracked as M14 in the remediation plan.

## Verified smoke path

The 2026-07-11 branch was exercised through a real isolated daemon and the signed-in Slack client:

- help/status/workspace/profile/session/unknown commands returned through the stdio plugin path;
- a real Codex two-turn conversation on the extended Slack route reused one session and recalled a token supplied only in turn one;
- unloading the agent host and sending a follow-up resumed the recorded session without replaying old IM output;
- a long-running prompt followed by `va stop` completed as ACP `cancelled`, produced no late agent text, and did not render an error;
- Discord reached gateway `ClientReady` after correcting its login/probe order, but the existing browser DM still received Discord/Clyde delivery rejection, so Discord end-to-end DM delivery is not marked passed.

---

*Source anchors: `src/core/src/channels/` (types, plugin_runner, transport_stdio, plugin_host, outbox, bridge_handler, prompt/), `src/server/src/lib.rs` (input dispatcher and shutdown), `src/core/src/workspace/manager.rs` + `threads/runtime.rs` (thread resolution, agent lifecycle).*
*Last verified: `codex/im-acp-route-refactor` at `de10c0e0`; Channel SDK `b55f3c0`; Slack `a0e6fe7`; Discord `d5e1f93`; Feishu `64fd81c`; Telegram `2143f6c` (2026-07-11).*

<sub>[◀ Internals](../README.md) · [Documentation index](../../README.md) · [Flow: web chat ▶](web-chat.md)</sub>
