# Module: tunnels

`src/core/src/tunnels/` (current location) — publishing the server's web listener to a public URL through four interchangeable providers.

## Responsibility

Start, track, and stop tunnel runtimes; expose the current public URL to the rest of the system (pairing hints, preview link bases). Tunnel is a **server exposure capability**, not a general core domain: its target is the bound web listener and its trust policy is coupled to server auth/origin enforcement. Provider process mechanics may remain reusable below that service.

## Key types

| Type | File | Role |
|---|---|---|
| `TunnelManager` | `mod.rs` | Registry of live tunnels: provider → URL, registry id, abort handle; implements `StateSource` |
| `start_web_tunnel_with_provider` | `mod.rs` | Entry: config → provider start → (guard, public URL) |
| ngrok provider | `providers/ngrok.rs` | Spawns the ngrok agent (`ngrok http`); URL parsed from its JSON logs; optional reserved domain |
| cloudflare provider | `providers/cloudflare.rs` | Child process: `cloudflared tunnel run --token …` |
| localtunnel provider | `providers/localtunnel.rs` | Child process: currently `npx localtunnel --port 12358` (known target-port defect) |
| tailscale provider | `providers/tailscale.rs` | Child process: `tailscale funnel --yes http://127.0.0.1:12358` |

## Interactions

- **← server (daemon boot):** starts the configured tunnel; reports Tailscale's `awaiting_approval` state; registers the abort handle; `stop()` aborts and clears.
- **← auth:** a public hostname is what triggers the pairing gate.
- **← previews / dashboard:** the active tunnel URL supplies paired Server/Markdown owner links and code-gated Share links.
- **→ resources:** provider program definitions and spawn-error hints (e.g. "is Node/npx installed?").

## Invariants — do not break

1. **`none` is a first-class provider** — no tunnel code runs, no child spawns; new call sites must tolerate absent URLs.
2. **The tunnel exposes exactly the web listener** — never bind additional ports through it; loopback-only surfaces (local-api) must stay unreachable.
3. **Server Share proxying stays page-oriented** — forward authenticated GET/HEAD paths unchanged, including page data reads. Writes, protocol upgrades, service workers, WebSockets, and HMR remain unsupported; `/va/*`, owner pages, chat, and review remain excluded. This is not an API-isolation sandbox; accepted GET/HEAD paths are not classified by name.
4. Provider children are registered for cleanup like every other child; a dead daemon leaves no `cloudflared` or `tailscale funnel` process behind.
5. Public URL is data, not identity: consumers subscribe to changes rather than caching it across restarts.

## Known debt

- Localtunnel currently hardcodes port 12358 instead of receiving the daemon's actual bound port; custom-port tunnel startup must be rejected until fixed.
- Provider startup/exit still needs an explicit `Starting` state, URL invalidation, and bounded backoff; `Running`, `AwaitingApproval`, `Failed`, and `Stopped` are already represented.
- Move orchestration behind an injected server `TunnelService`; core should not decide what local listener is safe to expose.

---

*Source anchors: `src/core/src/tunnels/` (mod, providers/), `src/core/src/config.rs` (tunnel settings), `src/server/src/lib.rs` (boot wiring).*
*Last verified: 2026-07-22.*

<sub>[◀ Module: previews](previews.md) · [Documentation index](../../README.md) · [Module: auth ▶](auth.md)</sub>
