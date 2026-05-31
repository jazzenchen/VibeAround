/**
 * Wire types for the Launch tab — must mirror the serde shapes emitted by
 * `src/desktop/src/profiles/`. ProfileSummary uses camelCase (matches the
 * Rust `#[serde(rename_all = "camelCase")]`); everything else is
 * snake_case to match the catalog JSON the user can read on disk.
 */

export type AuthMode = "api_key" | "oauth_via_cli";
export type CompatibilityBridgeMode = "auto" | "on" | "off";
export type ConnectionAgentId = "claude" | "codex" | "gemini" | "opencode" | "pi";

export interface ProfileSummary {
  id: string;
  label: string;
  provider: string;
  providerLabel: string;
  providerIcon: string | null;
  authMode: AuthMode;
  apiTypes: string[];
  launchTargets: LaunchTargetSummary[];
  /** `api_type → caveat string`. Populated only for api_types whose
   * catalog endpoint has a `compatibility_warning`. UI shows ⚠ on the
   * matching launch button. */
  apiTypeWarnings: Record<string, string>;
  apiTypeModels: Record<string, string>;
  apiTypeModelOptions: Record<string, ModelDef[]>;
  apiTypeHeaders: Record<string, Record<string, string>>;
}

export interface LaunchTargetSummary {
  id: string;
  label: string;
  apiType: string;
  warning?: string | null;
}

export interface ProfileConnectionPreference {
  selectedApiType?: string | null;
  bridge?: Record<string, ProfileBridgePreference | undefined> | null;
}

export interface ProfileBridgePreference {
  enabled?: boolean | null;
  targetApiType?: string | null;
  /** TODO(0.7.x): remove single-model compatibility fields after migration to models[]. */
  upstreamModel?: string | null;
  fakeModelId?: string | null;
  models?: ProfileBridgeModelPreference[] | null;
  headers?: Record<string, string> | null;
}

export interface ProfileBridgeModelPreference {
  upstreamModel?: string | null;
  fakeModelId?: string | null;
}

export type ProfileConnections = Record<
  string,
  Partial<Record<ConnectionAgentId, ProfileConnectionPreference>>
>;

export interface ApiTypeOverrides {
  endpoint_id?: string | null;
  base_url?: string | null;
  model?: string | null;
  reasoning_effort?: string | null;
  capabilities?: ContentCapabilities | null;
}

export interface ProviderSettings {
  deepseek?: DeepSeekProviderSettings | null;
}

export interface AgentLaunchPreference {
  profileId?: string | null;
  workspace?: string | null;
  launchArgs?: AgentLaunchArgs | null;
}

export interface AgentLaunchArgs {
  terminal?: string[] | null;
  acp?: string[] | null;
}

export interface DeepSeekProviderSettings {
  thinking?: boolean | null;
  replay_reasoning_content?: boolean | null;
}

export interface ProfileDef {
  id: string;
  label: string;
  provider: string;
  auth_mode: AuthMode;
  api_types: string[];
  credentials: Record<string, string>;
  overrides: Record<string, ApiTypeOverrides>;
  use_settings_proxy?: boolean;
  provider_settings?: ProviderSettings;
}

export type ProfileDraft = Omit<ProfileDef, "id">;

export interface ModelDef {
  id: string;
  label?: string | null;
  aliases?: string[] | null;
  context_window?: number | null;
  capabilities?: ContentCapabilities | null;
}

export interface FieldDef {
  name: string;
  label: string;
  secret: boolean;
  required: boolean;
  placeholder?: string | null;
  validate?: string | null;
}

export interface AuthModeDef {
  mode: string;
  label?: string | null;
  fields: FieldDef[];
  // `render` is a tagged-pass-through — the UI never needs to introspect
  // it, so we keep it as `unknown` to discourage drift with the renderer.
  render?: unknown | null;
}

export interface EndpointDef {
  id?: string | null;
  label?: string | null;
  api_type: string;
  default_base_url: string;
  append_v1_path?: boolean | null;
  headers?: Record<string, string> | null;
  auth_header?: boolean | null;
  models: ModelDef[];
  capabilities?: EndpointCapabilities | null;
  auth_modes: AuthModeDef[];
  compatibility_warning?: string | null;
}

export interface EndpointCapabilities {
  reasoning_effort?: boolean | null;
  content?: ContentCapabilities | null;
}

export interface ContentCapabilities {
  image_input?: boolean | null;
  file_input?: boolean | null;
}

export interface CatalogEntry {
  id: string;
  label: string;
  icon: string | null;
  homepage: string | null;
  endpoints: EndpointDef[];
}

/** Pretty-print an internal api_type token as the provider API kind. */
export function apiTypeLabel(api_type: string): string {
  switch (api_type) {
    case "anthropic":
      return "Anthropic API";
    case "openai-chat":
      return "OpenAI-compatible Chat";
    case "openai-responses":
      return "OpenAI Responses";
    case "gemini":
      return "Gemini GenerateContent";
    default:
      return api_type;
  }
}

/** Short API kind pill label inside provider/profile forms. */
export function apiTypeShort(api_type: string): string {
  switch (api_type) {
    case "anthropic":
      return "anthropic";
    case "openai-chat":
      return "openai-chat";
    case "openai-responses":
      return "responses";
    case "gemini":
      return "gemini";
    default:
      return api_type;
  }
}

export function apiTypeBadge(api_type: string): string {
  switch (api_type) {
    case "anthropic":
      return "anthropic";
    case "openai-chat":
      return "chat";
    case "openai-responses":
      return "responses";
    case "gemini":
      return "gemini";
    default:
      return api_type;
  }
}

export function isProviderApiKind(api_type: string): boolean {
  return ["anthropic", "openai-responses", "openai-chat", "gemini"].includes(api_type);
}
