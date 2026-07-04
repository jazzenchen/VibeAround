# CLI reference

The `va` command (alias: `vibearound`) is installed by `npm i @vibearound/cli`. It talks to a local or remote daemon over HTTP/WS.

Global flags: `--auth-file PATH`, `--base-url URL`, `--token TOKEN`, `--json`.

## Diagnostics

| Command | Purpose |
|---|---|
| `va help` | Show usage |
| `va health` | Check public server liveness |
| `va info` | Show server metadata |
| `va status` | Compact runtime summary |
| `va doctor` | Diagnose endpoint, auth, and server health |

## Server and auth

| Command | Purpose |
|---|---|
| `va serve` | Start the standalone VibeAround server |
| `va auth status` / `va auth clear` | Show / remove saved auth |
| `va pair start [--wait --save]` | Start browser/IM pairing (optionally wait and save auth) |
| `va pair status SID [--save]` / `va pair wait SID [--save]` | Poll / wait for pairing verification |
| `va settings reload` | Re-read settings.json |

## Chat

| Command | Purpose |
|---|---|
| `va chat send TEXT` | Send one prompt over `/ws/chat`, wait for completion |
| `va chat send --stdin` | Read the prompt from standard input |
| `va chat send --continue TEXT` | Resume the saved chat session for this workspace |
| `va chat repl` | Line-oriented chat session |
| `va chat sessions` / `va chat forget [--all]` | List / forget locally saved chat sessions |

## Channels and tunnels

| Command | Purpose |
|---|---|
| `va channels` | List channel plugin runtimes |
| `va channel sync` | Reconcile plugins with settings.json |
| `va channel start\|stop\|restart KIND` | Plugin lifecycle |
| `va tunnels` / `va tunnel kill PROVIDER` | List / stop tunnel runtimes |

## Agents and launches

| Command | Purpose |
|---|---|
| `va agents` | List enabled agents |
| `va agent kill ROUTE_KEY` | Kill an attached agent runtime |
| `va launch --profile NAME` / `--profile-path PATH` | Native agent launch (`--dry-run` validates only) |
| `va launch sessions` | List resumable native sessions |
| `va launch archive\|unarchive --agent A ID` | Archive / unarchive a launch session |

## PTY sessions and tmux

| Command | Purpose |
|---|---|
| `va sessions` | List PTY sessions |
| `va session create --tool TOOL [--attach]` | Create/resume a PTY session |
| `va session attach SESSION_ID` / `va session kill SESSION_ID` | Attach / kill |
| `va pty kill SESSION_ID` | Kill a PTY process by session id |
| `va tmux sessions` | List attachable tmux sessions |

## Workspaces, previews, profiles

| Command | Purpose |
|---|---|
| `va workspaces` | List registered workspaces |
| `va workspace add\|remove\|default PATH` | Manage workspace registry |
| `va workspace create NAME` | Create a workspace under the default root |
| `va previews` / `va preview delete SLUG` | List / close live previews |
| `va profiles` | List model profiles |

`vibearound tui` opens the terminal UI on the same contract.

---

*Source anchors: `src/cli/src/args.rs` (command enum and usage text — this page mirrors it 1:1).*
*Last verified: v0.7.11*
