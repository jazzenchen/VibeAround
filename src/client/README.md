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
