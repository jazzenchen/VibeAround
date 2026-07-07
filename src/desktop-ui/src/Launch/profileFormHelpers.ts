import type {
  ApiTypeOverrides,
  AuthMode,
  AuthModeDef,
  CatalogEntry,
  ContentCapabilities,
  FieldDef,
  ProfileApiConfig,
  ProfileHeaderConfig,
  ProfileModelConfig,
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

export function selectedAuthModes(
  provider: CatalogEntry,
  apiTypes: string[],
  overrides: Record<string, ApiTypeOverrides> = {},
): AuthModeDef[] {
  let common: AuthModeDef[] | null = null;
  for (const apiType of apiTypes) {
    const endpoint = selectedEndpoint(provider, apiType, overrides);
    if (!endpoint) continue;
    if (common == null) {
      common = [...endpoint.auth_modes];
      continue;
    }
    common = common.filter((auth) =>
      endpoint.auth_modes.some((candidate) => candidate.mode === auth.mode),
    );
  }
  return common ?? [];
}

export function normalizeAuthMode(
  mode: string | null | undefined,
): AuthMode | null {
  return mode === "api_key" ||
    mode === "oauth_via_cli" ||
    mode === "google_oauth"
    ? mode
    : null;
}

export function defaultAuthMode(
  provider: CatalogEntry,
  apiTypes: string[],
  overrides: Record<string, ApiTypeOverrides> = {},
  preferred?: AuthMode | null,
): AuthMode {
  const modes = selectedAuthModes(provider, apiTypes, overrides)
    .map((auth) => normalizeAuthMode(auth.mode))
    .filter((mode): mode is AuthMode => !!mode);
  if (preferred && modes.includes(preferred)) return preferred;
  if (modes.includes("api_key")) return "api_key";
  return modes[0] ?? "api_key";
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
  const next: ApiTypeOverrides = {
    ...current,
    endpoint_id: endpointId(endpoint),
    base_url: endpoint.default_base_url || undefined,
  };
  if (endpoint.models.length === 0) {
    next.model = current?.model ?? "";
  } else {
    delete next.model;
  }
  delete next.reasoning_effort;
  return next;
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

export function apiConfigForEndpoint(
  endpoint: CatalogEntry["endpoints"][number],
  current: ProfileApiConfig | undefined,
  override: ApiTypeOverrides | undefined,
): ProfileApiConfig {
  const selectedModel =
    cleanString(override?.model) ??
    cleanString(current?.model) ??
    endpoint.models[0]?.id ??
    "";
  const capabilities = override?.capabilities ?? current?.capabilities ?? undefined;
  return {
    ...current,
    enabled: current?.enabled ?? true,
    endpoint_id: endpointId(endpoint),
    base_url:
      override?.base_url ?? current?.base_url ?? endpoint.default_base_url ?? undefined,
    append_v1_path: current?.append_v1_path ?? endpoint.append_v1_path ?? true,
    model: selectedModel || undefined,
    reasoning_effort: override?.reasoning_effort ?? current?.reasoning_effort ?? undefined,
    capabilities,
    headers: current?.headers?.length
      ? current.headers
      : defaultHeaderConfigs(endpoint),
    models: current?.models?.length
      ? current.models
      : defaultModelConfigs(endpoint, selectedModel, capabilities),
  };
}

export function syncApiConfigsForProvider(
  provider: CatalogEntry,
  selectedApiTypes: string[],
  overrides: Record<string, ApiTypeOverrides>,
  current: Record<string, ProfileApiConfig> = {},
): Record<string, ProfileApiConfig> {
  const selected = new Set(selectedApiTypes);
  const out: Record<string, ProfileApiConfig> = {};
  for (const endpoint of providerApiKindEndpoints(provider)) {
    const apiType = endpoint.api_type;
    const selectedEndpointForType = selectedEndpoint(provider, apiType, overrides) ?? endpoint;
    const existing = current[apiType];
    if (!selected.has(apiType) && !existing) continue;
    out[apiType] = {
      ...apiConfigForEndpoint(
        selectedEndpointForType,
        existing,
        overrides[apiType],
      ),
      enabled: selected.has(apiType),
    };
  }
  return out;
}

function defaultHeaderConfigs(
  endpoint: CatalogEntry["endpoints"][number],
): ProfileHeaderConfig[] {
  const headers: ProfileHeaderConfig[] = Object.entries(endpoint.headers ?? {}).map(
    ([name, value]) => ({
      name,
      value,
      enabled: true,
      locked: true,
    }),
  );
  const authHeader = defaultAuthHeader(endpoint);
  if (
    authHeader &&
    !headers.some((header) => header.name.toLowerCase() === authHeader.name.toLowerCase())
  ) {
    headers.push(authHeader);
  }
  return headers;
}

function defaultAuthHeader(
  endpoint: CatalogEntry["endpoints"][number],
): ProfileHeaderConfig | null {
  if (endpoint.api_type === "openai-chat" || endpoint.api_type === "openai-responses") {
    return {
      name: "Authorization",
      value: "Bearer $apiKey",
      enabled: true,
      locked: true,
    };
  }
  if (endpoint.api_type === "anthropic" && endpoint.auth_header) {
    return {
      name: "Authorization",
      value: "Bearer $apiKey",
      enabled: true,
      locked: true,
    };
  }
  if (endpoint.api_type === "anthropic") {
    return {
      name: "x-api-key",
      value: "$apiKey",
      enabled: true,
      locked: true,
    };
  }
  if (endpoint.api_type === "gemini") {
    return {
      name: "x-goog-api-key",
      value: "$apiKey",
      enabled: true,
      locked: true,
    };
  }
  return null;
}

function defaultModelConfigs(
  endpoint: CatalogEntry["endpoints"][number],
  selectedModel: string,
  capabilities: ContentCapabilities | undefined,
): ProfileModelConfig[] {
  const models: ProfileModelConfig[] = endpoint.models.map((model) => ({
    id: model.id,
    label: model.label ?? undefined,
    enabled: true,
    context_window: model.context_window ?? undefined,
    capabilities: model.capabilities ?? {},
    custom: false,
  }));
  const model = selectedModel.trim();
  if (model && !models.some((item) => catalogModelMatches(endpoint, item.id, model))) {
    models.unshift({
      id: model,
      enabled: true,
      capabilities: capabilities ?? {},
      custom: true,
    });
  }
  return models;
}

function catalogModelMatches(
  endpoint: CatalogEntry["endpoints"][number],
  modelId: string,
  requested: string,
): boolean {
  const catalogModel = endpoint.models.find((model) => model.id === modelId);
  if (!catalogModel) return modelId === requested;
  return (
    catalogModel.id === requested ||
    (catalogModel.aliases ?? []).some((alias) => alias.trim() === requested)
  );
}

function cleanString(value: string | null | undefined): string | undefined {
  const trimmed = value?.trim();
  return trimmed ? trimmed : undefined;
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
  if (!endpoint.default_base_url) return true;
  return !!overrides.base_url && overrides.base_url !== endpoint.default_base_url;
}

export function requiresProfileModel(
  provider: CatalogEntry,
  endpoint: CatalogEntry["endpoints"][number] | undefined,
): boolean {
  return !!endpoint && (provider.id === "custom" || endpoint.models.length === 0);
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
    if (ov.model && ov.model.length > 0) {
      if (requiresProfileModel(provider, ep)) {
        trimmed.model = ov.model;
      } else if (
        ep?.models.some((model) => model.id === ov.model) &&
        ov.model !== ep.models[0]?.id
      ) {
        trimmed.model = ov.model;
      }
    }
    if (
      canOverrideInputSupport(provider, ep) &&
      (ov.capabilities?.image_input || ov.capabilities?.file_input || ov.capabilities?.web_search)
    ) {
      trimmed.capabilities = {
        ...(ov.capabilities?.image_input ? { image_input: true } : {}),
        ...(ov.capabilities?.file_input ? { file_input: true } : {}),
        ...(ov.capabilities?.web_search ? { web_search: true } : {}),
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
