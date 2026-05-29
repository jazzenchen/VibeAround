import type {
  ConnectionAgentId,
  ProfileConnectionPreference,
  ProfileConnections,
  ProfileBridgePreference,
  ProfileBridgeModelPreference,
  ProfileSummary,
} from "./types";
import { apiTypeBadge } from "./types";

export interface ConnectionAgentDef {
  id: ConnectionAgentId;
  label: string;
  supportedApiTypes: string[];
  defaultApiType: string;
}

export interface ResolvedClientApiConnection {
  apiType: string;
  native: boolean;
  bridgeEnabled: boolean;
  targetApiType: string | null;
  upstreamModel: string | null;
  fakeModelId: string | null;
  models: ProfileBridgeModelPreference[];
  agentModel: string | null;
  targetOptions: string[];
  status: "native" | "via_bridge" | "unsupported";
}

export interface ResolvedProfileConnection {
  agent: ConnectionAgentDef;
  selectedApiType: string;
  selected: ResolvedClientApiConnection;
  clientApiTypes: ResolvedClientApiConnection[];
  targetOptions: string[];
  status: "native" | "via_bridge" | "unsupported";
}

export const CONNECTION_AGENTS: ConnectionAgentDef[] = [
  {
    id: "claude",
    label: "Claude Code",
    supportedApiTypes: ["anthropic"],
    defaultApiType: "anthropic",
  },
  {
    id: "codex",
    label: "Codex CLI",
    supportedApiTypes: ["openai-responses"],
    defaultApiType: "openai-responses",
  },
  {
    id: "pi",
    label: "Pi",
    supportedApiTypes: ["anthropic", "openai-responses", "openai-chat"],
    defaultApiType: "anthropic",
  },
  {
    id: "gemini",
    label: "Gemini CLI",
    supportedApiTypes: ["gemini"],
    defaultApiType: "gemini",
  },
  {
    id: "opencode",
    label: "OpenCode",
    supportedApiTypes: ["openai-responses", "openai-chat", "anthropic"],
    defaultApiType: "openai-responses",
  },
];

const PROXY_TARGET_API_TYPES = ["anthropic", "openai-responses", "openai-chat"];

export function resolveProfileConnection(
  profile: ProfileSummary,
  connections: ProfileConnections | undefined,
  agent: ConnectionAgentDef,
): ResolvedProfileConnection {
  const preference = connections?.[profile.id]?.[agent.id];
  const targetOptions = bridgeTargetApiTypes(profile);
  const selectedApiType = selectedClientApiType(profile, agent, preference);
  const clientApiTypes = agent.supportedApiTypes.map((apiType) =>
    resolveClientApiConnection(profile, agent, preference, apiType, targetOptions),
  );
  const selected =
    clientApiTypes.find((item) => item.apiType === selectedApiType) ?? clientApiTypes[0];
  const status = selected?.status ?? "unsupported";

  return {
    agent,
    selectedApiType: selected?.apiType ?? selectedApiType,
    selected,
    clientApiTypes,
    targetOptions,
    status,
  };
}

export function emptyConnectionDraft(
  profile: ProfileSummary,
  connections: ProfileConnections | undefined,
): Record<ConnectionAgentId, ProfileConnectionPreference> {
  return Object.fromEntries(
    CONNECTION_AGENTS.map((agent) => {
      const resolved = resolveProfileConnection(profile, connections, agent);
      const bridge: Record<string, ProfileBridgePreference> = {};
      for (const item of resolved.clientApiTypes) {
        const current = connections?.[profile.id]?.[agent.id]?.bridge?.[item.apiType];
        bridge[item.apiType] = {
          enabled: item.bridgeEnabled,
          targetApiType:
            current?.targetApiType && item.targetOptions.includes(current.targetApiType)
              ? current.targetApiType
              : item.targetApiType,
          upstreamModel: current?.upstreamModel ?? item.upstreamModel,
          fakeModelId: current?.fakeModelId ?? item.fakeModelId,
          models:
            cleanBridgeModels(current?.models).length > 0
              ? cleanBridgeModels(current?.models)
              : defaultBridgeModels(profile, item.targetApiType, item.upstreamModel, item.fakeModelId),
          headers: current?.headers ?? {},
        };
      }
      return [
        agent.id,
        {
          selectedApiType: resolved.selectedApiType,
          bridge,
        },
      ];
    }),
  ) as Record<ConnectionAgentId, ProfileConnectionPreference>;
}

export function bridgeTargetApiTypes(profile: ProfileSummary): string[] {
  return profile.apiTypes.filter((apiType) => PROXY_TARGET_API_TYPES.includes(apiType));
}

export function recommendedClientApiType(
  profile: ProfileSummary,
  agent: ConnectionAgentDef,
): string {
  return (
    agent.supportedApiTypes.find((apiType) => profile.apiTypes.includes(apiType)) ??
    agent.defaultApiType
  );
}

export function recommendedBridgeTarget(
  profile: ProfileSummary,
  agentId: ConnectionAgentId,
  clientApiType: string,
): string | null {
  const order =
    agentId === "gemini" && clientApiType === "gemini"
      ? ["openai-chat", "openai-responses", "anthropic"]
      : (agentId === "claude" && clientApiType === "anthropic") ||
          (agentId === "opencode" && clientApiType === "anthropic") ||
          (agentId === "pi" && clientApiType === "anthropic")
        ? ["openai-responses", "openai-chat", "anthropic"]
        : ["anthropic", "openai-chat", "openai-responses"];
  return order.find((apiType) => profile.apiTypes.includes(apiType)) ?? null;
}

export function apiTypeProtocolLabel(apiType: string): string {
  switch (apiType) {
    case "anthropic":
      return "Anthropic Messages";
    case "openai-responses":
      return "OpenAI Responses";
    case "openai-chat":
      return "OpenAI Chat Completions";
    case "gemini":
      return "Gemini GenerateContent";
    default:
      return apiType;
  }
}

export function apiTypeRouteLabel(apiType: string): string {
  switch (apiType) {
    case "anthropic":
      return "Anthropic";
    case "openai-responses":
      return "Responses";
    case "openai-chat":
      return "Chat";
    default:
      return apiTypeBadge(apiType);
  }
}

function resolveClientApiConnection(
  profile: ProfileSummary,
  agent: ConnectionAgentDef,
  preference: ProfileConnectionPreference | undefined,
  apiType: string,
  targetOptions: string[],
): ResolvedClientApiConnection {
  const bridgePreference = preference?.bridge?.[apiType];
  const targetApiType =
    bridgePreference?.targetApiType && targetOptions.includes(bridgePreference.targetApiType)
      ? bridgePreference.targetApiType
      : recommendedBridgeTarget(profile, agent.id, apiType);
  const native = profile.apiTypes.includes(apiType);
  const bridgeEnabled = Boolean(bridgePreference?.enabled && targetApiType);
  const status = bridgeEnabled ? "via_bridge" : native ? "native" : "unsupported";
  const upstreamModel =
    cleanModelId(bridgePreference?.upstreamModel) ??
    (targetApiType ? cleanModelId(profile.apiTypeModels[targetApiType]) : null);
  const fakeModelId = cleanModelId(bridgePreference?.fakeModelId);
  const models = cleanBridgeModels(bridgePreference?.models);

  return {
    apiType,
    native,
    bridgeEnabled,
    targetApiType,
    upstreamModel,
    fakeModelId,
    models:
      models.length > 0
        ? models
        : defaultBridgeModels(profile, targetApiType, upstreamModel, fakeModelId),
    agentModel: fakeModelId ?? upstreamModel,
    targetOptions,
    status,
  };
}

export function defaultBridgeModels(
  profile: ProfileSummary,
  targetApiType: string | null,
  upstreamModel?: string | null,
  fakeModelId?: string | null,
): ProfileBridgeModelPreference[] {
  const preferred = cleanModelId(upstreamModel);
  const fake = cleanModelId(fakeModelId);
  const models: ProfileBridgeModelPreference[] = [];
  if (preferred) {
    models.push({ upstreamModel: preferred, fakeModelId: fake });
  }
  if (targetApiType) {
    for (const model of profile.apiTypeModelOptions[targetApiType] ?? []) {
      const id = cleanModelId(model.id);
      if (!id || models.some((entry) => cleanModelId(entry.upstreamModel) === id)) {
        continue;
      }
      models.push({ upstreamModel: id });
    }
  }
  return models.length > 0 ? models : [{ upstreamModel: "" }];
}

export function cleanBridgeModels(
  models: ProfileBridgeModelPreference[] | null | undefined,
): ProfileBridgeModelPreference[] {
  return (models ?? [])
    .map((model) => ({
      upstreamModel: cleanModelId(model.upstreamModel),
      fakeModelId: cleanModelId(model.fakeModelId),
    }))
    .filter((model) => model.upstreamModel);
}

function cleanModelId(value: string | null | undefined): string | null {
  const trimmed = value?.trim();
  return trimmed ? trimmed : null;
}

function selectedClientApiType(
  profile: ProfileSummary,
  agent: ConnectionAgentDef,
  preference: ProfileConnectionPreference | undefined,
): string {
  const selected = preference?.selectedApiType;
  if (!selected || !agent.supportedApiTypes.includes(selected)) {
    return recommendedClientApiType(profile, agent);
  }
  if (profile.apiTypes.includes(selected)) {
    return selected;
  }
  const bridgePreference = preference?.bridge?.[selected];
  const bridgeTarget = bridgePreference?.targetApiType;
  if (
    bridgePreference?.enabled &&
    bridgeTarget &&
    bridgeTargetApiTypes(profile).includes(bridgeTarget)
  ) {
    return selected;
  }
  return recommendedClientApiType(profile, agent);
}
