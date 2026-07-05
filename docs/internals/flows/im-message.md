# Flow: IM message

One message in a Telegram/Feishu/Slack chat, followed from platform event to streamed reply. This is the trunk flow — [web chat](web-chat.md) and [permission](permission.md) branch off it. File references are repo-relative; line-level detail lives in the anchored files' module docs.

## Hop by hop

```text
platform ─1─► plugin ─2─► stdio ─3─► input queue ─4─► shard worker
                                                          │5
                              ┌───────────────────────────┘
                              ▼
                     command? ──yes──► workspace-thread command handler
                        │no
                        ▼6                 ▼7                  ▼8
                  resolve route ──► ensure agent+session ──► ACP prompt
                                                              │
              chat ◄─10─ plugin ◄─ ChannelOutput ◄─9─ notifications
```

**1. Platform → plugin.** The channel plugin (own Node.js process) receives the webhook/long-poll event, downloads any attachments into `~/.vibearound/.cache/`, and builds a `ChannelEnvelope`: route key `(channel_kind, bot_id, chat_id)`, message id, sender, text, attachment refs.
→ plugin repo; envelope type in `src/core/src/channels/types.rs`

**2. Plugin → daemon.** The envelope crosses stdio as a JSON-RPC notification (ACP framing) and is decoded by the plugin bridge into `ChannelInput::Message`.
→ `src/core/src/channels/transport_stdio/`, `plugin_bridge.rs`

**3. Enqueue.** `ChannelManager::handle_input` is fire-and-forget: the input goes onto an unbounded mpsc queue. Nothing platform-facing ever blocks on agent work.
→ `src/core/src/channels/mod.rs` (`handle_input`)

**4. Shard dispatch.** The input loop hashes the route key onto one of 64 worker tasks. Same route → same worker → strict FIFO; different routes run in parallel. This is the ordering guarantee for one chat.
→ `src/server/src/lib.rs` (`channel_input_shard`, worker loop)

**5. Command parse.** The text is checked against the slash-command grammar (`/new`, `/close`, `/switch`, `/pickup`, `/status`, resource commands, `/va` prefix forms). Commands are executed against the workspace-thread layer and answered with system texts — the flow ends here for them.
→ `src/core/src/channels/prompt/handler.rs` (`parse_thread_command`, `handle_command`)

**6. Route → thread runtime.** `resolve_route_runtime` looks up the route's attachment: attached open thread → its runtime; no attachment → create a default workspace, persist a new thread event, attach the route. The default workspace for an IM route is `<default_workspace>/im/<channel_kind>` (path separators in the kind sanitized to `_`); the host agent/profile come from `remote.channels.<kind>` defaults, falling back to the global default. A per-route lock makes this idempotent under concurrency.
→ `src/core/src/workspace/manager.rs` (`resolve_route_runtime`)

**7. Ensure agent + session.** The thread runtime spawns its host agent if absent: profile env materialized, `VIBEAROUND_*` context env injected, process registered with the supervisor (restart policy `Never`), ACP `initialize` exchanged. Then it ensures a CLI session — resuming the thread's recorded session id when there is one.
→ `src/core/src/workspace/threads/runtime.rs` (`ensure_agent`, `ensure_session`)

**8. Prompt.** Text + attachment resource links become ACP content blocks; `session/prompt` is sent. The turn is serialized per thread (a second message on the same thread waits for the turn to finish).
→ `runtime.rs` (`prompt`), `src/core/src/channels/prompt/mod.rs` (content blocks)

**9. Notifications → outputs.** Every ACP `session_notification` from the agent is wrapped as a thread reply and fanned out as `ChannelOutput` to **every route attached to the thread** — which is why a handover listener sees the same turn live.
→ `src/core/src/channels/bridge_handler.rs` (`session_notification`)

**10. Output → chat.** `PluginHost` routes each output to the owning plugin's live runtime. Durable kinds (system texts, permission requests) are staged in the outbox first and replayed if the plugin is down. The plugin renders platform-native messages.
→ `src/core/src/channels/plugin_host.rs` (`send_output`), `outbox.rs`

**Epilogue.** After the turn: `PromptDone` (typing indicator off), errors sent as `❌` system text (auth errors auto-close the thread), and a 10-minute idle shutdown scheduled for the host agent. The thread and its session id persist; the next message transparently respawns.
→ `src/core/src/channels/prompt/mod.rs` (`handle_prompt_input`), `manager.rs` (idle shutdown)

## Failure behavior along the path

| Failure | Result |
|---|---|
| Plugin crashes before step 2 | Platform may redeliver; supervisor respawns the plugin |
| Daemon restarts between 4 and 8 | The in-flight turn is lost; thread + session persist and resume |
| Agent spawn fails at 7 | `❌` system text; auth-required errors auto-close the thread |
| Agent crashes mid-turn at 8 | Turn errors out; next prompt spawns fresh and resumes the session |
| Plugin dead at 10 | Durable outputs wait in the outbox for the respawned plugin † |

> † Known gap: this holds only while no runtime is registered for the channel. In the crash window the dead runtime is still routable, so durable outputs are marked sent (queued into the dead bridge) or nacked-and-dropped instead of waiting. Tracked as M14 in the remediation plan.

---

*Source anchors: `src/core/src/channels/` (types, transport_stdio, plugin_host, outbox, bridge_handler, prompt/), `src/server/src/lib.rs` (sharding), `src/core/src/workspace/manager.rs` + `threads/runtime.rs` (thread resolution, agent lifecycle).*
*Last verified: v0.7.11*

<sub>[◀ Internals](../README.md) · [Documentation index](../../README.md) · [Flow: web chat ▶](web-chat.md)</sub>
