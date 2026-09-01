# Build from source

Build any part of VibeAround — the standalone server, the CLI tools, or the full desktop app — from a checkout. This is the supported path for macOS Intel and for development.

## Prerequisites

- **Rust** (stable toolchain) — the workspace has seven crates
- **Bun** — drives the JS builds and workspace scripts
- **Node.js** — runtime for channel plugins and agent ACP adapters
- **Tauri system dependencies** — only for the desktop app ([Tauri prerequisites](https://tauri.app/start/prerequisites/) per platform: Xcode CLT on macOS, MSVC on Windows, webkit2gtk on Linux)

All build commands below run from `src/` in the repository.

## Standalone server + dashboard

```bash
bun install
bun run web:build        # dashboard SPA → web/dist
cargo build --release -p server
```

Run it with the built dashboard:

```bash
cargo run --release -p server    # binds 127.0.0.1:12358
```

## CLI, TUI, and launcher

```bash
bun run va:build         # cargo build -p va-cli -p va-tui -p va-launcher
```

Binaries land in `target/debug/` (or `--release`): `va`, `va-tui`, `va-launch`. Note `va launch` execs a sibling `va-launch` binary — keep them in the same directory, as the packaged distributions do.

## Desktop app

```bash
bun install
bun run build            # desktop-ui + web SPA, then tauri build
```

Development mode with hot reload for the UI:

```bash
bun run dev              # tauri dev (desktop-ui served by vite)
```

The Tauri build prepares `va-launch` and `va-tui` as sidecar binaries automatically (`scripts/prepare-va-launch.mjs` copies them into `src/desktop/binaries/`, which is gitignored) and bundles per-platform packages (DMG/EXE/MSI/AppImage/deb). A debug build additionally builds the channel plugin checkouts under `src/plugins/` via `scripts/build-project-plugins.mjs`.

## Running tests

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

JS surfaces build-check with `bun run web:build` and `bun run desktop-ui:build`.

## What you cannot build without maintainer secrets

Release packaging beyond a local unsigned build uses maintainer-private configuration that is deliberately not in the repository:

- **macOS signing/notarization** (`src/apple-sign.config`) — local DMGs work unsigned but Gatekeeper will warn.
- **Registry publishing** — `@vibearound/cli` is published by a GitHub Actions workflow using a repository secret; Windows and Linux release packages are built by CI on a tag push.

Everything needed for a fully functional local build is public; only distribution signing and registry credentials are private.

## The built-in agent (va-agent)

`va-agent` lives in its own repository ([jazzenchen/va-agent](https://github.com/jazzenchen/va-agent)) and is not part of the Cargo or Bun workspace. You do not need to build it: VibeAround installs the pinned `@vibearound/agent` release from npm on first use, the same way it installs the other ACP adapters.

To run a local build instead:

```bash
git clone https://github.com/jazzenchen/va-agent
cd va-agent && npm install && npm run build   # → dist/va-agent.js
```

Then set `VIBEAROUND_VA_AGENT_PATH` to the absolute path of `dist/va-agent.js`. The override takes precedence over the npm copy; without it there is no `PATH` lookup.

## Plugins and SDK

Channel plugins and `@vibearound/plugin-channel-sdk` are separate repositories with their own npm-based builds (npm, not bun, in those repos). See [Build a channel plugin](build-a-channel-plugin.md).

---

*Source anchors: `src/package.json` (build scripts), `src/Cargo.toml` (workspace members), `src/scripts/prepare-va-launch.mjs` (sidecar), `src/npm/cli/` (npm packaging).*
*Last verified: v0.7.24*

<sub>[◀ Build a channel plugin](build-a-channel-plugin.md) · [Documentation index](../README.md) · [Troubleshooting and FAQ ▶](troubleshooting-and-faq.md)</sub>
