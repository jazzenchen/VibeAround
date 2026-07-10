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

**2. Plugin → daemon.** The SDK sends stdio JSON-RPC/ACP. `ChannelPluginRunner` owns one protocol generation; the transport decodes `agent/prompt` and optional `va.channel` metadata into a `ChannelInput`. Legacy plugins without metadata still work, but default `bot_id/actor_id` values do **not** constitute multi-bot support.
→ `src/core/src/channels/plugin_runner.rs`, `transport_stdio/`, `types.rs`

**3. Enqueue.** `ChannelManager::handle_input` is fire-and-forget: the input goes onto an unbounded mpsc queue. Nothing platform-facing ever blocks on agent work.
→ `src/core/src/channels/mod.rs` (`handle_input`)

**4. Route lane.** `ConversationIngress` places prompt-bearing work into a bounded FIFO lane keyed by the complete `RouteKey` (capacity 16). Same route is serialized; different routes run independently without hash-collision head-of-line blocking. `Stop` increments the lane's stop generation, cancels the active runtime and discards older queued work. Daemon shutdown closes ingress first and waits for all lane tasks to drain.
→ `src/core/src/channels/prompt/ingress.rs`

**5. Command parse.** The text is checked against the slash-command grammar (`/new`, `/close`, `/switch`, `/pickup`, `/status`, resource commands, `/va` prefix forms). The core address policy is a defense-in-depth check: group text must be `Mention`-addressed and a callback is an explicit interaction; bare group commands are rejected. Commands execute against the workspace-thread layer and answer with system texts.
→ `src/core/src/channels/prompt/handler.rs` (`parse_thread_command`, `handle_command`)

**6. Route → thread runtime.** `resolve_route_runtime` looks up the route's attachment: attached open thread → its runtime; no attachment → create a default workspace, persist a new thread event, attach the route. Because `actor_id` and `topic_id` are part of the route, two addressed actors in one group can map to distinct threads when plugins provide that metadata. Official plugins do not yet populate it end to end, so this remains a staged contract rather than a shipped multi-bot guarantee.
→ `src/core/src/workspace/manager.rs` (`resolve_route_runtime`)

**7. Ensure agent + session.** `ThreadRuntime` keeps durable session identity separately from an `AcpSessionRunner`, which owns the live `Agent` and handler for one generation. A dead generation is replaced as a unit. Agent spawn is registered with the supervisor (`Never` restart policy), performs ACP initialize, then creates or resumes the recorded CLI session.
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
| Plugin crashes before step 2 | Platform may redeliver; supervisor respawns the plugin |
| Daemon restarts between 4 and 8 | Ingress closes before thread/process teardown; the in-flight turn is cancelled, while thread + session persist |
| Agent spawn fails at 7 | `❌` system text; auth-required errors auto-close the thread |
| Agent crashes mid-turn at 8 | Turn errors out; next prompt spawns fresh and resumes the session |
| Plugin dead at 10 | Durable outputs wait in the outbox for the respawned plugin † |

> † Known gap: this holds only while no runtime is registered for the channel. In the crash window the dead runtime is still routable, so durable outputs are marked sent (queued into the dead bridge) or nacked-and-dropped instead of waiting. Tracked as M14 in the remediation plan.

---

*Source anchors: `src/core/src/channels/` (types, plugin_runner, transport_stdio, plugin_host, outbox, bridge_handler, prompt/), `src/server/src/lib.rs` (input dispatcher and shutdown), `src/core/src/workspace/manager.rs` + `threads/runtime.rs` (thread resolution, agent lifecycle).*
*Last verified: `codex/im-acp-route-refactor` at `0ba7fa2e` (2026-07-11).*

<sub>[◀ Internals](../README.md) · [Documentation index](../../README.md) · [Flow: web chat ▶](web-chat.md)</sub>
