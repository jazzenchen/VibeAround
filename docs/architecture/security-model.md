# Security model

VibeAround exposes powerful things — a terminal, a coding agent with shell access, your provider credentials — so its trust boundaries are worth understanding precisely. The short version: everything runs on your machine, localhost is trusted after token auth, anything reaching in from outside must pair, and credentials never leave the daemon.

## Trust zones

```text
zone 0  the daemon process        holds credentials, spawns agents
zone 1  localhost clients         desktop app, local browser, va CLI — token auth
zone 2  tunneled browsers         pairing code + token required
zone 3  IM platforms              messages only, mediated by plugins and permission cards
```

## Zone 1: local clients and the auth token

Every daemon start generates a fresh random bearer token, written to `~/.vibearound/auth.json`. All protected HTTP and WebSocket routes require it, as `Authorization: Bearer <token>` or a `?token=` query parameter. The desktop app opens the dashboard with the token appended; the `va` CLI reads the token file; a browser that presents a stale token after a daemon restart is rejected and must reload through an authorized entry point.

Practical consequences:

- Another local user (or malware) cannot drive your agents just because the port is open — it needs the token file, which lives in your home directory.
- Restarting the daemon invalidates every previously issued URL.

## Zone 2: remote access through tunnels

Tunnels (ngrok, localtunnel, Cloudflare) publish the dashboard to a public URL. Two gates apply:

1. **Pairing.** A browser on a non-local hostname must complete pairing: the dashboard shows a 6-digit code that expires after 60 seconds, and the code must be confirmed from an already-trusted surface — typed into a connected IM chat (`/pair <code>`) or approved locally. Pairing binds that browser to the daemon's current auth token.
2. **Token.** After pairing, the same bearer-token rules apply as locally.

Local hostnames (`localhost`, `127.0.0.1`, `::1`, and the desktop app's own origins) skip pairing but never skip the token.

## Zone 3: IM platforms

IM messages arrive through channel plugins. The platform never gets a shell; it gets a conversation:

- **Permission cards.** When an agent wants to run a command or edit files beyond its allowed scope, the permission request is rendered as an interactive card in the chat. The agent's turn blocks until someone taps a choice. If the plugin dies mid-request, the request is cancelled rather than silently approved.
- **Route isolation.** Each chat maps to its own thread and workspace; a group chat cannot see or steer another chat's agent.
- **Attachment hygiene.** File keys from plugins are validated against path traversal (`..`, separators) before becoming file references; unsafe keys are dropped.
- **Bot credentials** (platform tokens) live in `settings.json` on your machine and are passed only to the owning plugin process.

## Previews: sharing without exposing

Live previews of dev servers and rendered Markdown get two URL forms:

| URL | Audience | Lifetime |
|---|---|---|
| `/preview/u/<slug>` | Owner (token-authenticated) | While the preview exists |
| `/preview/s/<slug>` | Anyone with the link | 600 seconds, then expires |

Share links are the only intentionally unauthenticated surface, scoped to one preview and time-boxed to 10 minutes. Everything else on the tunnel still requires pairing + token.

## Credential handling

- Provider API keys are stored in the profile store under `~/.vibearound/` and injected into upstream requests **by the daemon**. Rendered agent configs contain only local bridge URLs — a leaked agent config exposes no provider key.
- The bridge and agent-as-API endpoints are loopback-only and sit behind a local-bridge gate; they are not reachable through tunnels.
- The daemon's own auth token file is plaintext in your home directory (same trust level as `~/.ssh`); treat backups accordingly.
- Bridge request/response recording for the launch popup is held in memory only and never written to disk.

## What VibeAround does not protect against

Honest limits of the model:

- **A malicious local root/user account.** Anyone who can read your home directory has your token and credentials.
- **Malicious agent behavior within granted permissions.** Permission cards gate escalations, but a command you approve runs with your user's full rights.
- **IM platform compromise.** If someone controls your Telegram account, they can talk to your agent as you — with permission cards as the remaining barrier. Prefer per-chat bots and private chats for sensitive work.

---

*Source anchors: `src/server/src/web_server/auth.rs` (token middleware, local-origin rules), `src/core/src/auth/` (token file, pairing TTL), `src/core/src/previews/store.rs` (SHARE_TTL_SECS), `src/core/src/routing.rs` (attachment key validation), `src/server/src/web_server/mod.rs` (route protection layout).*
*Last verified: v0.7.11*

<sub>[◀ Local API and bridge](local-api-and-bridge.md) · [Documentation index](../README.md) · [Configuration reference ▶](../reference/configuration.md)</sub>
