# Desktop app guide

The desktop app (Tauri) is the management shell around the daemon: it embeds the same server the CLI runs, adds GUI screens for everything configurable, and keeps the runtime alive from the tray. If the daemon is the engine, the desktop app is the dashboard *and* the ignition.

## Service management

- The app starts the embedded daemon on launch and shows its health. Restarting the service from the tray/app performs a clean shutdown (agents, plugins, previews all wound down) and a fresh start with a new auth token.
- **Tray/menu bar:** open the dashboard (pre-authenticated), restart the service, quit. Closing the main window keeps the daemon running.
- Port conflicts on restart are retried automatically on Windows, where the OS releases listeners lazily.

## Onboarding and the startkit

First launch walks through toolchain, agents, profiles, and channels — see [Install and onboarding](install-and-onboarding.md). Two pieces you may revisit later:

- **Toolchain.** `system` mode uses your own Node.js; `managed` mode has VibeAround install and maintain a private toolchain (helpful on machines where you cannot or will not manage Node globally). Switchable in settings.
- **Startkit.** Platform scripts that install prerequisites and agent CLIs; installation state is tracked and reported per item, so a partially provisioned machine shows exactly what is missing.

## Model profiles

The profile screens manage the full profile lifecycle — create from the provider catalog or as custom endpoints, edit credentials and models, reorder, delete. Changes apply immediately to new launches and host switches; no daemon restart. Details and pairing advice: [Model profiles guide](model-profiles.md).

## Agent Launch

The Launch screen renders agent + workspace + profile into a native terminal launch, with terminal preference (Terminal.app/iTerm2/PowerShell/Linux terminals) and per-agent argument defaults. It also lists **resumable sessions** — including ones created outside VibeAround — with archive/unarchive controls. Details: [Agent launch guide](agent-launch.md).

Desktop-app agents (`claude-desktop`, `codex-desktop`) are detected separately (installed vendor apps) and launched as GUI apps with profile overlays where supported.

## Channels and plugins

The plugin manager installs, updates, and removes channel plugins; channel screens edit `channels.<kind>` config and control plugin lifecycle (start/stop/restart/sync) with live status from the supervisor — the GUI equivalent of `va channels` / `va channel *`. Platform-side setup steps: [Connect channels](connect-channels.md).

## Settings

The settings screens edit `~/.vibearound/settings.json` fields — workspaces, default agent, tunnel provider and credentials, proxy, search tool, integrations toggles ([Reference](../reference/configuration.md) documents every field). Editing the file by hand while the app runs is fine; use the reload action (or `va settings reload`) to apply.

## Where the desktop app is optional

Everything the app manages is also reachable headlessly: `va serve` + `settings.json` + the web dashboard covers servers and remote boxes. The app's unique value is the embedded lifecycle (no terminal babysitting), native launch integration, and guided onboarding.

---

*Source anchors: `src/desktop/src/` (main, tray, onboarding/, profiles/, startkit/), `src/desktop-ui/src/` (screens), `src/core/src/toolchain.rs` (system/managed), `src/desktop/src/desktop_detection.rs` (vendor apps).*
*Last verified: v0.7.11*

<sub>[◀ Quick tour](quick-tour.md) · [Documentation index](../README.md) · [Web dashboard guide ▶](web-dashboard.md)</sub>
