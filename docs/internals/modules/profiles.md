# Module: profiles

`src/core/src/profiles/` — model provider configuration: what a profile *is*, the built-in provider catalog, and how a profile becomes concrete env/config/URLs at launch time. The serving side of the bridge lives in [server](server.md); this module is the data and the rendering.

## Responsibility

Define the profile schema, ship the provider catalog, persist user profiles, and render (profile × agent × launch target) into the material a launch needs: environment variables, per-agent config overlays, alias model routes, and local-api base URLs.

## Key types

| Type | File | Role |
|---|---|---|
| `ProfileDef` | `schema.rs` | A stored profile: provider, endpoint, auth mode, models, routes |
| `catalog` | `catalog.rs` | Embedded provider catalog (12 providers × endpoints × models × capabilities), loaded from `src/resources/profile-catalog/` |
| `connections` | `connections/` | User profile store + model route resolution (`ProfileBridgeModelRoute`) |
| `render` / `RenderedProfile` | `render.rs` | Generic rendering: env targets, settings files, args |
| `bridge_launch` | `bridge_launch.rs` | Per-launch-target rendering: Claude/Codex/Gemini/opencode/pi (+desktop variants) config shapes, local-api URL minting |
| `runtime` | `runtime.rs` | Runtime lookups used while serving bridge requests |
| `google_oauth` | `google_oauth.rs` | Gemini OAuth/code-assist credential flow |
| `headers` | `headers.rs` | Merged upstream headers per provider |

## Interactions

- **← agent:** `agent/launch.rs` calls materialization for hosted spawns and native launches.
- **← server (api_bridge):** upstream endpoint resolution, model mapping inputs, capabilities for content policy.
- **← desktop / HTTP API:** profile CRUD.
- **→ config:** port and bridge settings for URL rendering.

## Invariants — do not break

1. **Credentials never leave the daemon**: rendered client configs carry local URLs and alias ids only; real keys are attached upstream-side. Any new render target must preserve this.
2. **Catalog is data**: new providers/models are catalog JSON, not code — except genuinely new auth flows (like google_oauth) or dialect quirks (provider adapters).
3. **Alias model ids are per-profile stable** — launched CLIs persist them in their own configs; renaming breaks existing launches.
4. **`direct` is the absence of a profile**, not a profile with empty fields; checks go through `profile_uses_vibearound_credentials`.

## Known debt

- Upstream auth is static API key per profile; subscription/OAuth upstreams beyond the Gemini adapter need new adapters (known limitation, noted in the api-bridge memory).
- `bridge_launch.rs` is 1.3k lines of per-target rendering — candidate for per-target submodules when the next target lands.

---

*Source anchors: `src/core/src/profiles/` (schema, catalog, connections/, render, bridge_launch, runtime, google_oauth, headers), `src/resources/profile-catalog/`.*
*Last verified: system review 2026-07-11.*

<sub>[◀ Module: agent](agent.md) · [Documentation index](../../README.md) · [Module: previews ▶](previews.md)</sub>
