# va-launch Architecture Notes

This document records the current launch boundary so future changes do not mix
provider profile runtime concerns with native process launching.

## Core Boundary

`va-launch` is the native launch boundary. It should own host-local execution
concerns:

- launch profile input loading (`--profile`, `--profile-path`, `--input-file`)
- workspace resolution and validation
- agent executable resolution and validation
- terminal preference detection and validation
- native launch plan construction
- terminal/app process spawning

`va-launch` should not own VibeAround provider profile runtime concerns:

- provider profile selection and validation
- API bridge preparation
- credential/env materialization
- Codex Desktop profile overlay
- Claude Desktop profile overlay

Those provider/runtime concerns belong in shared VibeAround profile/runtime code
used by Desktop, CLI, and any future launch entrypoints.

## Two Different Profiles

There are two separate concepts named "profile":

- **Provider profile**: a VibeAround model/provider configuration, including
  credentials, API type, bridge behavior, and client-specific rendering.
- **Launch profile**: a native launch request shape for `va-launch`, including
  agent id, workspace, terminal choice, executable, args, session id, and window
  label.

The two may be connected by a resolver, but they are not the same data model.

## Current Flow

Desktop currently does this:

1. UI chooses a provider profile and launch target.
2. Desktop/core prepares provider runtime details: bridge, env, desktop overlay,
   profile materialization, workspace, and project integrations.
3. Desktop builds a materialized `NativeLaunchInput`.
4. Desktop invokes `va-launch --input-file <temp-json>`.
5. `va-launch` validates native launch details and spawns the terminal/app.

This means Desktop launches currently go through `va-launch`, but the provider
runtime work still happens before `va-launch`.

## Desired Direction

The desired shape is not to move provider runtime work into `va-launch` core.
Instead, factor provider/runtime launch preparation into shared code:

```text
Desktop / CLI
  -> shared provider launch resolver
  -> NativeLaunchInput
  -> va-launch native engine
```

In that shape, Desktop and CLI can pass simpler launch intent, while `va-launch`
remains focused on native execution. The `va-launch` binary may call the shared
resolver as an entrypoint convenience, but the resolver remains a VibeAround
profile/runtime layer, not native launcher logic.

## Migration Rule

When moving launch logic, ask:

- Does this need provider credentials, API bridge state, or client config
  overlays? Keep it in shared profile/runtime code.
- Does this need local filesystem, executable, terminal, or OS process behavior?
  Keep it in `va-launch`.

