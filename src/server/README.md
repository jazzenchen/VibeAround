# VibeAround Server

`vibearound-server` is the standalone Axum runtime for VibeAround. It owns the
runtime APIs, Web Hub, API bridge, IM/channel runtime, MCP endpoint, previews,
tunnels, sessions, settings, model profiles, and launcher planning.

Desktop may embed this server, but the server must also run without Tauri.

## Run

From `src/`:

```sh
cargo run -p server -- --port 12358 --web-dist web/dist
```

Useful options:

```text
--port <port>        loopback HTTP port
--data-dir <path>    settings/state directory
--web-dist <path>    built web dashboard dist directory
--auth-mode token    token auth; currently the only supported mode
```

Equivalent environment variables:

```text
VIBEAROUND_PORT
VIBEAROUND_DATA_DIR
VIBEAROUND_WEB_DIST
VIBEAROUND_AUTH_MODE
```

When `--data-dir` or `VIBEAROUND_DATA_DIR` is set, `settings.json`, `auth.json`,
logs, profiles, and runtime state are written under that directory.

## Management APIs

Open liveness:

```text
GET /va/api/service/health
```

Token-protected management APIs:

```text
GET  /va/api/service/info
GET  /va/api/settings
PUT  /va/api/settings          # requires If-Match from GET ETag
PATCH /va/api/settings         # RFC 6902 JSON Patch
POST /va/api/settings/reload

GET  /va/api/workspaces
POST /va/api/workspaces
POST /va/api/workspaces/create
POST /va/api/workspaces/remove
PUT  /va/api/workspaces/order
PUT  /va/api/workspaces/default

GET    /va/api/model-profiles
POST   /va/api/model-profiles
GET    /va/api/model-profiles/{id}
PUT    /va/api/model-profiles/{id}
DELETE /va/api/model-profiles/{id}
PUT    /va/api/model-profiles/order

GET  /va/api/launcher/preferences
PUT  /va/api/launcher/default-agent
PUT  /va/api/launcher/agent-profile
PUT  /va/api/launcher/agent-launch-args
PUT  /va/api/launcher/selected-agent
PUT  /va/api/launcher/local-agent-api
PUT  /va/api/launcher/profile-connection
POST /va/api/launcher/plan
```

The auth token is written to `<data-dir>/auth.json` on startup. Use it as a
bearer token for protected HTTP APIs.

## Smoke Check

```sh
DATA_DIR="$(mktemp -d)"
cargo run -p server -- --port 12358 --data-dir "$DATA_DIR" --web-dist web/dist
```

In another shell:

```sh
curl http://127.0.0.1:12358/va/api/service/health
TOKEN="$(jq -r .token "$DATA_DIR/auth.json")"
curl -H "Authorization: Bearer $TOKEN" \
  http://127.0.0.1:12358/va/api/service/info
```
