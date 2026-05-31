import type {
  ApiTypeOverrides,
  AuthModeDef,
  CatalogEntry,
  FieldDef,
  ProviderSettings,
} from "./types";
import { apiTypeLabel, apiTypeShort, isProviderApiKind } from "./types";

export interface ProviderEndpointGroup {
  id: string;
  label: string;
  endpoints: CatalogEntry["endpoints"];
}

/**
 * Walk the selected api_types and union their auth-mode-matching `fields[]`
 * by `name`. Two endpoints of the same provider should declare the same
 * field for a given credential, so this dedupes on the catalog side rather
 * than asking the user to re-enter the same api_key for each protocol.
 */
export function collectFields(
  provider: CatalogEntry,
  apiTypes: string[],
  mode: string,
  overrides: Record<string, ApiTypeOverrides> = {},
): FieldDef[] {
  const seen = new Map<string, FieldDef>();
  for (const apiType of apiTypes) {
    const ep = selectedEndpoint(provider, apiType, overrides);
    if (!ep) continue;
    const auth = ep.auth_modes.find((a: AuthModeDef) => a.mode === mode);
    if (!auth) continue;
    for (const f of auth.fields) {
      if (!seen.has(f.name)) seen.set(f.name, f);
    }
  }
  return Array.from(seen.values());
}

export function hostnameOf(url: string): string {
  try {
    return new URL(url).hostname;
  } catch {
    return url;
  }
}

export function providerSearchText(provider: CatalogEntry): string {
  const parts = [
    provider.id,
    provider.label,
    provider.homepage ?? "",
    ...provider.endpoints
      .filter((endpoint) => isProviderApiKind(endpoint.api_type))
      .flatMap((endpoint) => [
        endpointId(endpoint),
        endpoint.label ?? "",
        endpoint.api_type,
        apiTypeShort(endpoint.api_type),
        apiTypeLabel(endpoint.api_type),
      ]),
  ];
  return parts.join(" ").toLowerCase();
}

export function stripEmpty(map: Record<string, string>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(map)) {
    if (v) out[k] = v;
  }
  return out;
}

export function arraysEqual(a: string[], b: string[]): boolean {
  return a.length === b.length && a.every((item, index) => item === b[index]);
}

export function endpointId(endpoint: CatalogEntry["endpoints"][number]): string {
  return endpoint.id || endpoint.api_type;
}

export function endpointLabel(endpoint: CatalogEntry["endpoints"][number]): string {
  return endpoint.label || endpointId(endpoint);
}

export function providerEndpointGroups(provider: CatalogEntry): ProviderEndpointGroup[] {
  const groups = new Map<string, ProviderEndpointGroup>();
  for (const endpoint of provider.endpoints) {
    if (!isProviderApiKind(endpoint.api_type)) continue;
    const id = endpointId(endpoint);
    const existing = groups.get(id);
    if (existing) {
      existing.endpoints.push(endpoint);
    } else {
      groups.set(id, {
        id,
        label: endpointLabel(endpoint),
        endpoints: [endpoint],
      });
    }
  }
  return Array.from(groups.values());
}

export function providerUsesEndpointGroups(provider: CatalogEntry): boolean {
  if (provider.id === "custom") return false;
  const groups = providerEndpointGroups(provider);
  if (groups.length <= 1) return false;
  return groups.some((group) =>
    group.endpoints.some((endpoint) => endpoint.id || endpoint.label),
  );
}

export function defaultApiKindEndpoints(provider: CatalogEntry): CatalogEntry["endpoints"] {
  if (providerUsesEndpointGroups(provider)) {
    return providerEndpointGroups(provider)[0]?.endpoints ?? [];
  }
  return providerApiKindEndpoints(provider);
}

export function providerApiKindEndpoints(provider: CatalogEntry): CatalogEntry["endpoints"] {
  const seen = new Set<string>();
  const out: CatalogEntry["endpoints"] = [];
  for (const endpoint of provider.endpoints) {
    if (!isProviderApiKind(endpoint.api_type) || seen.has(endpoint.api_type)) continue;
    seen.add(endpoint.api_type);
    out.push(endpoint);
  }
  return out;
}

export function providerApiKindsEditable(provider: CatalogEntry): boolean {
  return (
    provider.id === "custom" ||
    provider.id === "dashscope" ||
    provider.id === "gemini" ||
    provider.id === "volcengine"
  );
}

export function selectedEndpointGroup(
  provider: CatalogEntry,
  apiTypes: string[],
  overrides: Record<string, ApiTypeOverrides>,
): ProviderEndpointGroup | undefined {
  const groups = providerEndpointGroups(provider);
  if (groups.length === 0) return undefined;
  for (const apiType of apiTypes) {
    const endpoint = selectedEndpoint(provider, apiType, overrides);
    if (!endpoint) continue;
    const group = groups.find((candidate) => candidate.id === endpointId(endpoint));
    if (group) return group;
  }
  return groups[0];
}

export function overrideForEndpoint(
  endpoint: CatalogEntry["endpoints"][number],
  current?: ApiTypeOverrides,
): ApiTypeOverrides {
  const currentModel = current?.model ?? "";
  const modelStillValid = endpoint.models.some((model) => model.id === currentModel);
  return {
    ...current,
    endpoint_id: endpointId(endpoint),
    base_url: endpoint.default_base_url || undefined,
    model: modelStillValid ? currentModel : (endpoint.models[0]?.id ?? currentModel),
  };
}

export function overridesForEndpoints(
  endpoints: CatalogEntry["endpoints"],
  current: Record<string, ApiTypeOverrides> = {},
): Record<string, ApiTypeOverrides> {
  const next = { ...current };
  for (const endpoint of endpoints) {
    next[endpoint.api_type] = overrideForEndpoint(
      endpoint,
      current[endpoint.api_type],
    );
  }
  return next;
}

export function endpointsForApiType(
  provider: CatalogEntry,
  apiType: string,
): CatalogEntry["endpoints"] {
  return provider.endpoints.filter((endpoint) => endpoint.api_type === apiType);
}

export function selectedEndpoint(
  provider: CatalogEntry,
  apiType: string,
  overrides: Record<string, ApiTypeOverrides>,
): CatalogEntry["endpoints"][number] | undefined {
  const endpointIdOverride = overrides[apiType]?.endpoint_id;
  const candidates = endpointsForApiType(provider, apiType);
  return (
    candidates.find((endpoint) => endpointId(endpoint) === endpointIdOverride) ??
    candidates[0]
  );
}

export function shouldShowBaseUrl(
  provider: CatalogEntry,
  endpoint: CatalogEntry["endpoints"][number],
  overrides: ApiTypeOverrides,
): boolean {
  if (provider.id === "custom") return true;
  if (provider.id === "mimo" && endpointId(endpoint).startsWith("token-plan")) {
    return true;
  }
  if (!endpoint.default_base_url) return true;
  return !!overrides.base_url && overrides.base_url !== endpoint.default_base_url;
}

export function apiKindHint(
  provider: CatalogEntry,
  apiType: string,
  endpoint?: CatalogEntry["endpoints"][number],
): string | undefined {
  if (provider.id === "mimo" && endpoint && endpointId(endpoint).startsWith("token-plan")) {
    return "Token Plan keys must use the Base URL shown on the MiMo Subscription page.";
  }
  if (provider.id === "gemini" && apiType === "openai-chat") {
    if (endpoint && endpointId(endpoint) === "vertex-openai-compatible") {
      return "Uses a Google Cloud access token and a Vertex endpoint root ending in /endpoints/openapi.";
    }
    return "Uses the same Gemini API key through Google AI Studio's OpenAI-compatible Chat Completions surface.";
  }
  if (provider.id === "gemini" && apiType === "gemini") {
    return "Uses the native Gemini GenerateContent API with a Gemini API key.";
  }
  if (provider.id === "volcengine" && endpoint) {
    if (endpointId(endpoint) === "coding-plan") {
      return "Coding Plan uses subscription Base URLs; /api/v3 belongs to pay-as-you-go Ark API.";
    }
    if (endpointId(endpoint) === "agent-plan") {
      return "Agent Plan requires its dedicated API key; Ark API and Coding Plan keys cannot be reused.";
    }
    return "Ark API uses pay-as-you-go API keys and versioned Model IDs.";
  }
  if (provider.id !== "azure") return undefined;
  if (apiType === "openai-responses") {
    return "Used by Codex and OpenCode for reasoning/tools. Must be an Azure deployment that supports the Responses API.";
  }
  if (apiType === "openai-chat") {
    return "Chat Completions fallback for CLIs/providers that cannot use Responses.";
  }
  return undefined;
}

/**
 * Strip override values that match the catalog default. This keeps
 * profile.json minimal and lets future catalog updates flow through
 * automatically.
 */
export function pruneOverrides(
  overrides: Record<string, ApiTypeOverrides>,
  apiTypes: string[],
  provider: CatalogEntry,
): Record<string, ApiTypeOverrides> {
  const out: Record<string, ApiTypeOverrides> = {};
  for (const apiType of apiTypes) {
    const ov = overrides[apiType];
    if (!ov) continue;
    const ep = selectedEndpoint(provider, apiType, overrides);
    const endpointOptions = endpointsForApiType(provider, apiType);
    const defaultBaseUrl = ep?.default_base_url ?? "";
    const trimmed: ApiTypeOverrides = {};
    if (ov.endpoint_id && endpointOptions.length > 1) {
      trimmed.endpoint_id = ov.endpoint_id;
    }
    if (ov.model && ov.model.length > 0) trimmed.model = ov.model;
    if (ep?.capabilities?.reasoning_effort && ov.reasoning_effort) {
      trimmed.reasoning_effort = ov.reasoning_effort;
    }
    if (
      canOverrideInputSupport(provider, ep) &&
      (ov.capabilities?.image_input || ov.capabilities?.file_input)
    ) {
      trimmed.capabilities = {
        ...(ov.capabilities?.image_input ? { image_input: true } : {}),
        ...(ov.capabilities?.file_input ? { file_input: true } : {}),
      };
    }
    if (ov.base_url && ov.base_url.length > 0 && ov.base_url !== defaultBaseUrl) {
      trimmed.base_url = ov.base_url;
    }
    if (Object.keys(trimmed).length > 0) out[apiType] = trimmed;
  }
  return out;
}

export function canOverrideInputSupport(
  provider: CatalogEntry,
  endpoint: CatalogEntry["endpoints"][number] | undefined,
): boolean {
  return provider.id === "custom" || (endpoint?.models.length ?? 0) === 0;
}

export function pruneProviderSettings(
  providerId: string,
  settings: ProviderSettings,
): ProviderSettings {
  if (providerId !== "deepseek") return {};

  const deepseek = settings.deepseek ?? {};
  const trimmed = {
    ...(deepseek.thinking ? { thinking: true } : {}),
    ...(deepseek.replay_reasoning_content
      ? { replay_reasoning_content: true }
      : {}),
  };

  return Object.keys(trimmed).length > 0 ? { deepseek: trimmed } : {};
}
