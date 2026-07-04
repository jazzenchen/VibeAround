# Module: tunnels

`src/core/src/tunnels/` — publishing the local dashboard to a public URL through three interchangeable providers.

## Responsibility

Start, track, and stop tunnel runtimes; expose the current public URL to the rest of the system (pairing hints, preview link bases). Each provider has a different process model, hidden behind one start/guard interface.

## Key types

| Type | File | Role |
|---|---|---|
| `TunnelManager` | `mod.rs` | Registry of live tunnels: provider → URL, registry id, abort handle; implements `StateSource` |
| `start_web_tunnel_with_provider` | `mod.rs` | Entry: config → provider start → (guard, public URL) |
| ngrok provider | `providers/ngrok.rs` | In-process via the ngrok Rust SDK (session + forwarder; optional reserved domain) |
| cloudflare provider | `providers/cloudflare.rs` | Child process: `cloudflared tunnel run --token …` |
| localtunnel provider | `providers/localtunnel.rs` | Child process: `npx localtunnel --port 12358` |

## Interactions

- **← server (daemon boot):** starts the configured tunnel; registers the abort handle; `stop()` aborts and clears.
- **← auth:** a public hostname is what triggers the pairing gate.
- **← previews / dashboard:** public URL for share links and display (`preview_base_url` can override).
- **→ resources:** provider program definitions and spawn-error hints (e.g. "is Node/npx installed?").

## Invariants — do not break

1. **`none` is a first-class provider** — no tunnel code runs, no child spawns; new call sites must tolerate absent URLs.
2. **The tunnel exposes exactly the web listener** — never bind additional ports through it; loopback-only surfaces (local-api) must stay unreachable.
3. Provider children are registered for cleanup like every other child; a dead daemon leaves no `cloudflared` behind.
4. Public URL is data, not identity: consumers subscribe to changes rather than caching it across restarts.

## Known debt

- None tracked in the remediation plan. (Provider set is catalog-of-three by design; new providers follow the guard pattern.)

---

*Source anchors: `src/core/src/tunnels/` (mod, providers/), `src/core/src/config.rs` (tunnel settings), `src/server/src/lib.rs` (boot wiring).*
*Last verified: v0.7.11*
