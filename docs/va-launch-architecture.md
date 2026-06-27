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

## Current Flow

Desktop currently does this:

1. UI chooses a provider profile and launch target.
2. Desktop/core prepares provider runtime details: bridge, env, desktop overlay,
   profile materialization, workspace, and project integrations.
3. Desktop builds a materialized launch profile JSON.
4. Desktop invokes `va-launch --profile-path <launch-profile-json>`.
5. `va-launch` validates native launch details and spawns the terminal/app.

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
