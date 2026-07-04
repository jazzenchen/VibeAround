# Install and onboarding

Two ways to run VibeAround: the **desktop app** (recommended — GUI management plus the embedded server) or the **npm CLI** (headless server for terminal-first setups and remote machines). Both produce the same daemon with the same data directory, so you can switch later without losing anything.

## Option 1: desktop app

Download the package for your platform from the [GitHub releases page](https://github.com/jazzenchen/VibeAround/releases):

| Platform | Package |
|---|---|
| macOS Apple Silicon | DMG |
| Windows x64 | Setup EXE, MSI, or portable ZIP |
| Linux x64 | AppImage or deb |

macOS Intel is currently source-build only — see [Build from source](build-from-source.md).

Install and launch. The app lives in the tray/menu bar; closing the window does not stop the daemon.

### First-run onboarding

The desktop app walks you through setup on first launch:

1. **Toolchain check.** VibeAround needs Node.js for agent ACP adapters and channel plugins. Onboarding detects your system toolchain, or installs a managed one if you prefer not to touch the system (`toolchain` setting: `system` or `managed`).
2. **Agent detection.** Installed agent CLIs (Claude Code, Codex, Gemini CLI, …) are detected on PATH; you choose which to enable. Agents you enable get VibeAround's MCP endpoint and skills injected into their global config so hosted sessions have the integration tools available.
3. **Model profiles (optional).** Add a provider credential now, or skip and use agents with their own vendor logins (the `direct` profile).
4. **Channels (optional).** Install and configure IM channel plugins. Each channel needs platform-side setup (bot tokens etc.) covered in [Connect channels](connect-channels.md); you can do this any time later.

After onboarding, the dashboard opens in your browser, already authenticated. You are ready for the [Quick tour](quick-tour.md).

## Option 2: npm CLI

```bash
npm i -g @vibearound/cli
```

This installs the `va` and `vibearound` commands, the native server binaries, the `va-launch` native launcher, the TUI, and the pre-built web dashboard.

Start the server:

```bash
va serve
```

The daemon binds `127.0.0.1:12358`, writes its auth token to `~/.vibearound/auth.json`, and serves the dashboard at `http://127.0.0.1:12358/` (open it with the token — `va status` shows the URL). Configuration is `~/.vibearound/settings.json`, created with defaults on first run; there is no GUI onboarding in this mode, so channels and profiles are configured by editing settings (see [Reference](../reference/configuration.md)) or through the dashboard.

Useful first commands:

```bash
va status      # runtime summary: channels, tunnels, agents, sessions
va doctor      # diagnose endpoint, auth, and server health
va agents      # list enabled agents
vibearound tui # terminal UI
```

## The data directory

Everything VibeAround persists lives in `~/.vibearound/` (override with `VIBEAROUND_DATA_DIR`):

```text
~/.vibearound/
├── settings.json         # main configuration
├── auth.json             # dashboard auth token (regenerated each start)
├── agents.json           # detected agent executables
├── plugins/              # installed channel plugins
├── workspaces/           # default root for created workspaces
├── launch/profiles/      # saved va-launch profiles
└── *.jsonl               # workspace/thread/attachment event logs
```

Uninstalling the app or package never deletes this directory; remove it manually for a clean slate.

## Upgrading

- **Desktop:** install the new package over the old one; data directory is untouched.
- **npm:** `npm i -g @vibearound/cli@latest`.
- Check release notes for breaking changes before major upgrades — settings migrations run automatically on daemon start, but channel plugins may need `va channel sync` afterwards.

---

*Source anchors: `src/npm/cli/` (package contents), `src/core/src/config.rs` (data_dir, DEFAULT_PORT, settings bootstrap), `src/desktop/src/onboarding/` (onboarding steps), `src/core/src/toolchain.rs` (system/managed), `src/cli/src/args.rs` (va commands).*
*Last verified: v0.7.11*
