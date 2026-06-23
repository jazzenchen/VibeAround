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
- `--auth-file PATH`
- `$VIBEAROUND_AUTH_FILE`
- `$VIBEAROUND_DATA_DIR/auth.json`
- `~/.vibearound/auth.json`

## Examples

```bash
va health
va status
va --json status
va pair start
va pair status SID
va channels
va channel restart feishu
va workspaces
va workspace add /path/to/project
```
