# Tunnels and remote access

A tunnel publishes your dashboard to a public URL so you can reach it away from the machine — phone on the subway, laptop at a café. Three providers are built in; every remote browser must pair before it gets in. The trust rules behind this page are in [Security model](../architecture/security-model.md).

## Choosing a provider

| Provider | Setting value | Needs an account | Stable hostname |
|---|---|---|---|
| ngrok | `ngrok` | Yes (auth token) | With a reserved domain |
| localtunnel | `localtunnel` | No | No (random per start) |
| Cloudflare Tunnel | `cloudflare` | Yes (tunnel token) | Yes (your hostname) |
| disabled | `none` (default) | — | — |

Rules of thumb: **localtunnel** for zero-setup trials; **ngrok** for a personal stable URL with minimal config; **Cloudflare** for a permanent hostname on your own domain.

## Configuration

In [`~/.vibearound/settings.json`](../reference/configuration.md#settingsjson) (or the desktop settings screen):

```jsonc
{
  "tunnel_provider": "ngrok",
  "ngrok_auth_token": "2ab...",
  "ngrok_domain": "myname.ngrok.app"        // optional reserved domain
}
```

```jsonc
{
  "tunnel_provider": "cloudflare",
  "cloudflare_tunnel_token": "eyJ...",       // from the Zero Trust dashboard
  "cloudflare_hostname": "va.example.com"
}
```

```jsonc
{ "tunnel_provider": "localtunnel" }
```

The tunnel starts with the daemon. Status and the public URL show in the dashboard, `va tunnels`, and the desktop app; `va tunnel kill <provider>` stops one without restarting the daemon.

## First visit from a remote browser: pairing

Opening the public URL on a new device shows the pairing gate:

1. The browser displays a 6-digit code (valid 60 seconds, refreshable).
2. Confirm it from a surface you already trust:
   - type `/pair <code>` in any connected IM chat, or
   - approve it locally (dashboard/desktop on the machine), or
   - `va pair` flows from the CLI (`pair start --wait --save` also stores auth for CLI use against a remote daemon).
3. The browser is bound to the daemon's current auth token and behaves like a local one.

Pairing survives browser restarts but not daemon restarts (tokens are regenerated). Local origins (`localhost`, `127.0.0.1`, the desktop app) never see the gate.

## What a tunnel exposes — and what it never does

Through the tunnel, after pairing: the dashboard SPA, web chat, web terminal, previews, and the WebSocket endpoints — everything token-gated. Preview **share links** (`/preview/s/<slug>`) are the one deliberate exception: no pairing, no token, single preview, 10-minute expiry.

Never reachable through a tunnel: the local API bridge and agent-as-API endpoints (loopback-only), the MCP endpoint's local-bridge surface, and provider credentials in any form.

## Remote CLI

The `va` CLI can target a remote daemon: `va --base-url https://va.example.com --token <token> status`, or save auth once via the pairing flow (`va pair start --wait --save`) and use `va` normally. `--auth-file` points at an alternate saved auth.

## Troubleshooting

| Symptom | Check |
|---|---|
| Public URL never appears | Provider token invalid, or egress blocked — daemon logs show the tunnel error; `va tunnels` shows state |
| Pairing code always "invalid or expired" | Codes last 60 s — generate and confirm within the window; confirm you typed it in a chat connected to the *same* daemon |
| Everything 401s after a daemon restart | Expected: tokens regenerate — reload from a trusted entry point and re-pair remote browsers |
| localtunnel URL changes every start | That is localtunnel; use ngrok reserved domains or Cloudflare for stability |
| Web terminal sluggish remotely | Interactive PTY over long-haul tunnels is latency-bound — prefer web chat remotely, terminal locally |

---

*Source anchors: `src/core/src/tunnels/` (providers: ngrok, localtunnel, cloudflare), `src/core/src/config.rs` (tunnel settings), `src/core/src/auth/pair.rs` (60 s codes), `src/server/src/web_server/auth.rs` (local-origin trust), `src/core/src/previews/store.rs` (share TTL), `src/cli/src/` (pair/tunnel commands).*
*Last verified: v0.7.11*

<sub>[◀ Agent launch guide](agent-launch.md) · [Documentation index](../README.md) · [Build a channel plugin ▶](build-a-channel-plugin.md)</sub>
