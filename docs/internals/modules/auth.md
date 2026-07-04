# Module: auth

`src/core/src/auth/` — the two credentials that gate every surface: the per-boot daemon token and short-lived pairing codes. Policy discussion: [security model](../../architecture/security-model.md).

## Responsibility

Generate and persist the daemon auth token, and manage the pairing-code table that lets remote browsers earn that token. Enforcement (middleware) lives in [server](server.md); this module is the source of truth.

## Key types

| Type | File | Role |
|---|---|---|
| `AuthToken` | `token.rs` | Random per-daemon-start bearer token |
| `write_token_file` | `mod.rs` | Persists `{port, token}` to `~/.vibearound/auth.json` for out-of-process consumers (tray, CLI, desktop-ui) |
| `pair` | `pair.rs` | 6-digit codes, 60 s TTL, verified via a trusted surface; `validate(code)` returns the token on success |

## Interactions

- **← server:** `require_auth` middleware checks the token (header or `?token=`); the pairing HTTP flow drives code lifecycle.
- **← channels:** `/pair <code>` in a chat is a trusted confirmation path.
- **← cli:** `va pair` flows; `va auth` reads/clears the saved file.
- **← desktop:** reads the token file to open the dashboard pre-authenticated.

## Invariants — do not break

1. **Token rotates every daemon start** and the file is overwritten immediately — stale URLs must fail. Never persist a token across restarts.
2. **Pairing codes are one-shot-ish and 60 s** — purge-on-access keeps the table clean; a code never outlives its window.
3. **Confirmation must come from an already-trusted surface** (local origin or a connected chat). Adding a new confirmation path means adding a new trust assumption — think twice.
4. The token file is plaintext by design (home-directory trust level); nothing else secret goes in it.

## Known debt

- None tracked in the remediation plan.

---

*Source anchors: `src/core/src/auth/` (token, pair, mod), `src/server/src/web_server/auth.rs` (enforcement), `src/server/src/web_server/pair.rs` (HTTP flow).*
*Last verified: v0.7.11*
