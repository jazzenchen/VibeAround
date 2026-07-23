# Internals

Documentation for debugging VibeAround and changing its code. Two complementary cuts of the same runtime:

- **Flows** follow one request *through time* — every hop from entry to exit, with code anchors and failure tables. Start here to trace behavior.
- **Modules** describe one component *in space* — responsibility, key types, interactions, invariants you must not break, and known debt. Start here to modify code.

They cross-link where a flow passes through a module. The reader-level "why is it designed this way" story lives in [architecture/](../architecture/overview.md); hard numbers live in [timers and limits](../reference/timers-and-limits.md).

## Flows

| Flow | Path it follows |
|---|---|
| [IM message](flows/im-message.md) | Platform event → plugin runner → bounded route lane → thread → agent → streamed reply. **The trunk flow — read it first** |
| [Web chat](flows/web-chat.md) | WebSocket event → session intent → the same prompt path |
| [Permission](flows/permission.md) | Agent request → oneshot registration → card → tap → agent resumes |
| [Bridge request](flows/bridge-request.md) | Client dialect → decode → model mapping → upstream → streamed back |
| [Native launch](flows/native-launch.md) | Profile → launch JSON → va-launch → terminal spawn |
| [Handover](flows/handover.md) | Code issued → `/pickup` → external session bound → route attached |
| [Web terminal](flows/web-terminal.md) | Browser xterm ↔ WebSocket ↔ pseudo-tty |

## Modules

Fixed structure per page: responsibility · key types · interactions · invariants · known debt.

| Module | One-liner |
|---|---|
| [channels](modules/channels.md) | Message transport and routing between surfaces and threads |
| [workspace](modules/workspace.md) | Conversation state: workspaces, threads, attachments (event-sourced) |
| [process](modules/process.md) | Subprocess supervision: spawn, respawn, watchdog, cleanup |
| [agent](modules/agent.md) | One ACP connection to a coding CLI + launch preparation |
| [profiles](modules/profiles.md) | Provider catalog, profile store, launch rendering |
| [pty](modules/pty.md) | Pseudo-terminal sessions behind the web terminal |
| [previews](modules/previews.md) | Live preview registry, owner/share URLs |
| [tunnels](modules/tunnels.md) | ngrok / localtunnel / cloudflare / Tailscale Funnel publishing |
| [auth](modules/auth.md) | Daemon token and pairing codes |
| [server](modules/server.md) | The axum shell: routes, WebSockets, MCP, bridge, boot/shutdown |

## Subsystem deep-dives

Cross-cutting subsystems that span several modules get a dedicated page:

| Page | Covers |
|---|---|
| [Launch](launch.md) | The four launch paths, env assembly and injection per path, per-OS terminal handling, argument sources, desktop vs CLI producers |

## Related material

- Known defects and planned refactors are summarized in each module page's "Known debt" section and the current three-part system review under `reports/system-review-2026-07-10/`.
- Rustdoc module headers in the source are the finest-grained authority; these pages are maps, not replacements.

<sub>[◀ Provider endpoints reference](../reference/provider-endpoints.md) · [Documentation index](../README.md) · [Flow: IM message ▶](flows/im-message.md)</sub>
