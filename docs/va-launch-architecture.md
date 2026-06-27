# va-launch Architecture Notes

This document records the current launch boundary so future changes do not mix
provider profile runtime concerns with native process launching.

## Core Boundary

`va-launch` is the native launch boundary. It should own host-local execution
concerns:

- launch profile JSON loading (`--profile`, `--profile-path`)
- workspace resolution and validation
- agent executable resolution and validation
- terminal preference detection and validation
- project integration auto-install
- native launch plan construction
- terminal/app process spawning

`va-launch` should not own VibeAround provider profile runtime concerns:

- provider profile selection and validation
- API bridge preparation
- credential/env materialization
- Codex Desktop profile overlay
- Claude Desktop profile overlay

Those provider/runtime concerns belong upstream of `va-launch`. Desktop, CLI, or
another producer may perform that work, but the output handed to `va-launch`
must be a launch profile JSON file.

## Two Different Profiles

There are two separate concepts named "profile":

- **Provider profile**: a VibeAround model/provider configuration, including
  credentials, API type, bridge behavior, and client-specific rendering.
- **Launch profile**: a native launch request shape for `va-launch`, including
  agent id, workspace, terminal choice, executable, args, session id, and window
  label.

The two may be connected by a resolver, but they are not the same data model.

## Launch Profile JSON

`va-launch` accepts launch profile JSON from exactly two sources:

- `--profile <name>` reads
  `~/.vibearound/launch/profiles/<name>.json` (or
  `$VIBEAROUND_DATA_DIR/launch/profiles/<name>.json`).
- `--profile-path <path>` reads the specified JSON file.

The current schema version is `1`. Unknown fields are rejected so producers do
not silently hand `va-launch` a provider profile or another unrelated JSON
shape.

```json
{
  "schemaVersion": 1,
  "id": "openai-codex",
  "agent": "codex",
  "profileId": "openai",
  "launchTarget": "codex",
  "workspace": "/Users/example/project",
  "terminal": "terminal",
  "command": "codex",
  "executablePath": null,
  "windowLabel": "OpenAI Codex",
  "env": {
    "OPENAI_API_KEY": "..."
  },
  "args": {
    "native": ["--model", "gpt-5"]
  },
  "cleanupPaths": [],
  "macosAppProbe": null,
  "windowsProcessProbe": null
}
```

`profileId` is launch metadata from the upstream producer. It must not cause
`va-launch` to read VibeAround provider profile storage.

## Default Agent Executable

`va-launch` resolves an agent executable without Desktop or Server:

1. If the launch profile has `executablePath`, use and validate that path.
2. Otherwise, read `~/.vibearound/agents.json`
   (or `$VIBEAROUND_DATA_DIR/agents.json`) and use
   `agents.<agent>.executable.path` when present.
3. If no executable is configured for that agent, scan the current `PATH` for
   the command program, write the discovered path back to
   `agents.<agent>.executable`, and launch with that path.
4. Once an executable is configured, `va-launch` trusts that configuration and
   does not scan again. If the configured executable is invalid, launch fails
   with a validation error.

App launch wrappers such as `open -a ...` on macOS and `Start-Process ...` on
Windows are treated as native app commands, not agent CLI executables, and are
not written into `agents.json`.

## Project Integrations

On real launch, `va-launch` installs project-scoped integrations for the
resolved workspace using the shared VibeAround settings policy:

- MCP config follows `settings.json` `integrations.mcp_auto_install`.
- Skill files follow `settings.json` `integrations.skill_auto_install`.
- `codex-desktop` installs the companion `codex` project integrations.
- `claude-desktop` installs the companion `claude` project integrations.

`--dry-run` only builds and reports the native launch plan; it must not install
or mutate project files.

## Current Flow

Desktop currently does this:

1. UI chooses a provider profile and launch target.
2. Desktop/core prepares provider runtime details: bridge, env, desktop overlay,
   profile materialization, and workspace.
3. Desktop builds a materialized launch profile JSON.
4. Desktop invokes `va-launch --profile-path <launch-profile-json>`.
5. `va-launch` validates native launch details, installs project integrations,
   and spawns the terminal/app.

This means Desktop launches currently go through `va-launch`, but the provider
runtime work still happens before `va-launch`.

## Desired Direction

The desired shape is not to move provider runtime work into `va-launch`. Instead,
provider/runtime launch preparation must happen before `va-launch` is invoked:

```text
Desktop / CLI / another producer
  -> provider/runtime preparation when needed
  -> launch profile JSON
  -> va-launch --profile-path <json>
```

`va-launch` must not call VibeAround server, Desktop, or a shared provider
resolver. It must be able to launch from a launch profile JSON without
VibeAround Desktop or Server running.

## Migration Rule

When moving launch logic, ask:

- Does this need provider credentials, API bridge state, or client config
  overlays? Keep it in shared profile/runtime code.
- Does this need local filesystem, executable, terminal, or OS process behavior?
  Keep it in `va-launch`.
