# va-client

Pure Rust protocol helpers for VibeAround server consumers.

This crate is the Rust implementation of `@va/client` from the desktop/server
boundary plan. It owns request construction, response decoding, wire models,
and client-side error normalization for `@va/server`.

It deliberately does not own:

- HTTP transport implementation
- WebSocket transport implementation
- Tauri IPC
- native commands
- process spawning
- UI state

Desktop, CLI, and future TUI code should provide their own host transport and
pass `RequestSpec` / `ResponseSpec` values through this crate.

## Layout

- `auth`, `endpoint`, `http`, `operation`: shared protocol primitives.
- `service`, `settings`, `runtime`, `launcher`, `sessions`, `profiles`,
  `workspaces`, `previews`: per-domain request builders and wire models.
- `ops/`: ready-to-send operation catalog, grouped by server domain and
  re-exported from `va_client::ops`.
- `events/`: WebSocket specs, PTY client frames, and typed event decoders.
- `state/`: display-oriented reducers for CLI/TUI/desktop surfaces.
