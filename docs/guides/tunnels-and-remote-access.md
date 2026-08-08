# Tunnels and remote access

A tunnel publishes your dashboard to a public URL so you can reach it away from the machine — phone on the subway, laptop at a café. Four providers are built in; remote browsers must pair before entering protected dashboard and owner surfaces. Markdown shares use a separate six-digit access-code gate instead of owner pairing. The trust rules behind this page are in [Security model](../architecture/security-model.md).

## Choosing a provider

| Provider | Setting value | Needs an account | Stable hostname |
|---|---|---|---|
| ngrok | `ngrok` | Yes (auth token) | With a reserved domain |
| localtunnel | `localtunnel` | No | No (random per start) |
| Cloudflare Tunnel | `cloudflare` | Yes (tunnel token) | Yes (your hostname) |
| Tailscale Funnel | `tailscale` | Yes (signed-in Tailscale client) | Yes (`*.ts.net`) |
| disabled | `none` (default) | — | — |

Rules of thumb: **localtunnel** for zero-setup trials; **ngrok** for a personal stable URL with minimal config; **Cloudflare** for a permanent hostname on your own domain; **Tailscale Funnel** for a stable public URL when the host already uses Tailscale.

## Configuration

In [`~/.vibearound/settings.json`](../reference/configuration.md#settingsjson) (or the desktop settings screen):

```jsonc
{
  "tunnel": {
    "provider": "ngrok",
    "ngrok": {
      "auth_token": "2ab...",
      "domain": "myname.ngrok.app"          // optional reserved domain
    }
  }
}
```

```jsonc
{
  "tunnel": {
    "provider": "cloudflare",
    "cloudflare": {
      "tunnel_token": "eyJ...",             // from the Zero Trust dashboard
      "hostname": "va.example.com"
    }
  }
}
```

```jsonc
{ "tunnel": { "provider": "localtunnel" } }
```

```jsonc
{ "tunnel": { "provider": "tailscale" } }
```

### Tailscale Funnel needs a signed-in client

Install Tailscale, sign in to a tailnet, and enable MagicDNS. VibeAround starts `tailscale funnel --yes http://127.0.0.1:12358` as a foreground child process, reads the public `.ts.net` URL, and stops Funnel with the daemon.

The first start may require an owner or admin to approve Funnel in the Tailscale web console. VibeAround shows **Action required** with an **Enable Funnel** button; the approval page opens only when you click it, then startup continues after approval. No terminal command is required. Funnel is public: remote browsers do not need the Tailscale app, and VibeAround pairing remains required. See [Tailscale Funnel](https://tailscale.com/docs/features/tailscale-funnel) for tailnet requirements and platform limitations.

### Cloudflare needs one manual step

VibeAround starts `cloudflared tunnel run --token …` and uses your hostname for generated URLs — but it does **not** create the Cloudflare *Published application route*. In Zero Trust, add one for the same tunnel:

| Field | Value |
|---|---|
| Public hostname | The hostname configured in VibeAround, e.g. `vibe.example.com` |
| Path | Leave empty (match all paths) |
| Service | `HTTP` → `localhost:12358` |

Then open `https://vibe.example.com/va/` — the dashboard lives under `/va/`, so test that path, not just the root.

To isolate Cloudflare from VibeAround: stop VibeAround, serve anything on `127.0.0.1:12358` (`python3 -m http.server 12358 --bind 127.0.0.1`), and `curl -i https://vibe.example.com/`. A Cloudflare 404 before your temp server answers means the problem is in the hostname/DNS/route/tunnel-health layer, not VibeAround. References: [Published applications](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/routing-to-tunnel/), [run parameters](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/configure-tunnels/run-parameters/).

The tunnel starts with the daemon. Status and the public URL show in the dashboard, `va tunnels`, and the desktop app; `va tunnel kill <provider>` stops one without restarting the daemon.

## First visit from a remote browser: pairing

Opening the dashboard or an owner URL on a new device shows the pairing gate:

1. The browser displays a 6-digit code (valid 60 seconds, refreshable).
2. Confirm it from a surface you already trust:
   - type `/pair <code>` in any connected IM chat, or
   - approve it locally (dashboard/desktop on the machine), or
   - `va pair` flows from the CLI (`pair start --wait --save` also stores auth for CLI use against a remote daemon).
3. The browser is bound to the daemon's current auth token and behaves like a local one.

Pairing survives browser restarts but not daemon restarts (tokens are regenerated). Local origins (`localhost`, `127.0.0.1`, the desktop app) never see the gate.

## What a tunnel exposes — and what it never does

Through the tunnel, after pairing: the dashboard SPA, web chat, web terminal, Markdown owner previews, and the WebSocket endpoints — everything token-gated. Markdown **shares** (`/preview/s/<share_id>`) are the deliberate exception to owner pairing: viewers enter the reusable six-digit access code, then receive a path-scoped browser grant. The URL, code, and grant cover one document and expire together after 10 minutes. Live Server preview routes are local-only and return `403` on public hostnames.

Never reachable through a tunnel: the local API bridge and agent-as-API endpoints (loopback-only), the MCP endpoint's local-bridge surface, and provider credentials in any form.

## Remote CLI

The `va` CLI can target a remote daemon: `va --base-url https://va.example.com --token <token> status`, or save auth once via the pairing flow (`va pair start --wait --save`) and use `va` normally. `--auth-file` points at an alternate saved auth.

## Troubleshooting

| Symptom | Check |
|---|---|
| Public URL never appears | Provider token invalid, or egress blocked — daemon logs show the tunnel error; `va tunnels` shows state |
| Cloudflare: tunnel healthy but 404 | Missing/wrong Published application route — see the Cloudflare section above |
| Pairing code always "invalid or expired" | Codes last 60 s — generate and confirm within the window; confirm you typed it in a chat connected to the *same* daemon |
| Everything 401s after a daemon restart | Expected: tokens regenerate — reload from a trusted entry point and re-pair remote browsers |
| localtunnel URL changes every start | That is localtunnel; use ngrok reserved domains or Cloudflare for stability |
| Tailscale shows “Action required” without a URL | Click **Enable Funnel**, complete the Tailscale approval page, and confirm the Tailscale app is signed in |
| Tailscale exits before showing a URL | Run `tailscale funnel http://127.0.0.1:12358` manually to see whether this client/platform supports Funnel |
| Web terminal sluggish remotely | Interactive PTY over long-haul tunnels is latency-bound — prefer web chat remotely, terminal locally |

---

*Source anchors: `src/core/src/tunnels/` (providers: ngrok, localtunnel, cloudflare, tailscale), `src/core/src/config.rs` (tunnel settings), `src/core/src/auth/pair.rs` (60 s codes), `src/server/src/web_server/auth.rs` (local-origin trust), `src/core/src/previews/store.rs` (share TTL), `src/cli/src/` (pair/tunnel commands).*
*Last verified: v0.7.20*

<sub>[◀ Agent launch guide](agent-launch.md) · [Documentation index](../README.md) · [Build a channel plugin ▶](build-a-channel-plugin.md)</sub>
