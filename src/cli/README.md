# va-cli

Rust command-line client for VibeAround server management.

`va-cli` owns host-side transport and filesystem access. It uses `va-client`
for request construction and response decoding.

## Build

```bash
cd src
cargo build -p va-cli
./target/debug/va help
```

For a release binary:

```bash
cd src
cargo build -p va-cli --release
./target/release/va help
```

Package scripts are available from `src/`:

```bash
bun cli:dev help
bun cli:build
```

## Auth

Public commands such as `health` and `pair start` do not need a token.
Protected commands read auth from, in order:

- `--token` with optional `--base-url`
- `$VIBEAROUND_TOKEN` or `$VIBEAROUND_AUTH_TOKEN` with optional `$VIBEAROUND_BASE_URL`
- `--auth-file PATH`
- `$VIBEAROUND_AUTH_FILE`
- `$VIBEAROUND_DATA_DIR/auth.json`
- `~/.vibearound/auth.json`

## Examples

```bash
va health
va doctor
va --json doctor
va status
va --json status
VIBEAROUND_BASE_URL=http://127.0.0.1:12358/va VIBEAROUND_TOKEN=... va status
va pair start
va pair status SID
va channels
va channel restart feishu
va launch sessions
va launch sessions --agent codex --workspace /path/to/project --archived
va launch archive --agent codex SESSION_ID
va launch unarchive --agent codex SESSION_ID
va session create --tool codex --project /path/to/project
va session create --tool codex --project /path/to/project --attach
va session create --profile my-profile --target claude --project /path/to/project
va session create --tmux existing-tmux-session
va session attach SESSION_ID
va tmux sessions
va workspaces
va workspace add /path/to/project
```
