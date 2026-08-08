# @va/client

Hand-written TypeScript shared between the web dashboard (`src/web/`)
and the Tauri desktop-ui (`src/desktop-ui/`).

## What's here

- `src/routes.ts` — `VA_PREFIX` + base-URL builders. The single place
  that encodes the daemon's route layout (matches
  `.nest("/va", ...)` in `src/server/src/web_server/mod.rs`).
- `src/schemas.ts` — zod schemas + inferred types for all HTTP/WS
  wire shapes, plus the hand-maintained `AGENT_IDS` constant.
- `src/index.ts` — barrel.

## Relationship to core

Core (`src/core/`) owns the truth about domain shapes and the
JSON it emits via serde. It does **not** generate TypeScript. Each
consumer (this library, future TUI/CLI, curl scripts) validates at
its own wire boundary against schemas kept in its own language.

When you change a wire-facing Rust type (an `#[derive(Serialize)]`
struct or enum returned by a handler), update the matching zod schema
here in the same PR. The Rust doc comments on each wire type
(`src/server/src/api_types.rs`, `src/core/src/service/snapshot.rs`)
show the JSON shape — use them as the reference.

There is no codegen. There is no CI enforcement. Reviewers catch
drift; that's it. Drift in practice is rare because the PR that
changes the Rust shape usually also changes the TS consumer.
