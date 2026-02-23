# Build & packaging

All commands assume you are in `src/` (workspace root) unless noted.

## Four targets

| Target        | Dev command              | Build command              | Output / used by |
|---------------|---------------------------|----------------------------|------------------|
| **desktop-tray** | `bun run desktop-tray:dev` | `bun run desktop-tray:build` | `desktop-tray/dist` → **desktop** (Tauri window UI) |
| **web**       | `bun run web:dev`         | `bun run web:build`        | `web/dist` → **server** (SPA), **desktop** (spawns server with this path) |
| **server**    | `bun run server:dev`      | `bun run server:build`     | `target/debug/vibearound-server` or `target/release/` |
| **desktop**   | `bun run desktop:dev`     | `bun run desktop:build`   | `target/debug/vibearound-desktop` or Tauri bundle |

## Path references

- **Desktop → tray**: `tauri.conf.json` `frontendDist`: `../desktop-tray/dist`; `beforeBuildCommand` builds desktop-tray (and web).
- **Desktop → web**: At runtime, desktop spawns the server with `CARGO_MANIFEST_DIR/../web/dist` (i.e. `src/web/dist`). Run `bun run web:build` before running desktop so this path exists.
- **Server → web**: Runtime only. Default `--dist` is `web/dist` when run from `src/`. Override with `--dist <path>`.

## One-shot frontend build

```bash
bun run build
```

Builds **desktop-tray** and **web** (both dists). Do this before running desktop or server so `web/dist` and `desktop-tray/dist` exist.

## Desktop build (Tauri)

```bash
bun run desktop:build
```

Runs `beforeBuildCommand` (desktop-tray build + web build), then Tauri bundles the app using `desktop-tray/dist`. So tray and web dists are produced and the tray UI is bundled; at runtime the app still expects `../web/dist` (relative to the crate) for the in-process server.
