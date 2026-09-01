# CLI quick start

Use the npm CLI when you want a headless VibeAround daemon, a terminal-first setup, or a remote machine without the desktop app. The CLI runs the same server and uses the same `~/.vibearound/` data directory as the desktop build.

## 1. Install the CLI

```bash
npm i -g @vibearound/cli
va --help
```

If you already run the desktop app, you can still install the CLI. It will talk to the same local daemon when the desktop app is running.

## 2. Start the daemon

```bash
va serve
```

The daemon binds to `127.0.0.1:12358`, creates `~/.vibearound/settings.json` on first run, and writes a fresh local auth token to `~/.vibearound/auth.json`.

In another terminal:

```bash
va status
va doctor
```

`va status` prints the dashboard URL, runtime state, channels, tunnels, and active sessions. `va doctor` checks auth, endpoint reachability, and server health.

## 3. Open the TUI or browser dashboard

```bash
va tui
```

The TUI is the fastest terminal surface for checking runtime state. For the browser dashboard, use the authenticated URL from `va status`; it opens the same Web Chat, live previews, workspace list, profiles, and runtime controls as the desktop app.

## 4. Add a workspace and agent

```bash
va workspace add ~/dev/my-app
va workspaces
va agents
```

Agents must still be installed and authenticated in their own native way first: Claude Code, Codex CLI, Gemini CLI, OpenCode, and similar tools should work from a plain terminal before VibeAround hosts or launches them.

## 5. Launch or chat

Launch a native agent CLI in your own terminal:

```bash
va launch --profile my-codex-profile
```

Or send a single hosted Web Chat prompt:

```bash
va chat send "look at ~/dev/my-app and summarize the project"
```

For long-lived interactive work, use Web Chat, an IM channel, or the TUI. The full command list is in the [CLI reference](../reference/cli.md).

---

*Source anchors: `src/cli/src/args.rs` (CLI command surface), `src/server/src/lib.rs` (daemon), `src/core/src/config.rs` (settings/auth paths).*
*Last verified: v0.7.12*

<sub>[◀ Quick tour](quick-tour.md) · [Documentation index](../README.md) · [Desktop app guide ▶](desktop-app.md)</sub>
