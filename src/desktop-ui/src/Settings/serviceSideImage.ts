import type { Settings as AppSettings } from "../Onboarding/types";
import type { ProfileSummary } from "../Launch/types";

const SUPPORTED_API_TYPES = [
  "openai-chat",
  "openai-responses",
  "anthropic",
  "gemini",
] as const;

export type ServiceSideImageProfile = {
  key: string;
  id: string;
  label: string;
  apiType: string;
  models: string[];
};

export function buildServiceSideImageProfiles(
  profiles: ProfileSummary[],
): ServiceSideImageProfile[] {
  return profiles.flatMap((profile) => {
    if (profile.authMode !== "api_key") return [];

    return SUPPORTED_API_TYPES.flatMap((apiType) => {
      const models = Array.from(
        new Set(
          (profile.apiTypeModelOptions[apiType] ?? [])
            .filter((model) => model.capabilities?.image_input)
            .map((model) => model.id.trim())
            .filter(Boolean),
        ),
      );
      return models.length > 0
        ? [
            {
              key: JSON.stringify([profile.id, apiType]),
              id: profile.id,
              label: profile.label,
              apiType,
              models,
            },
          ]
        : [];
    });
  });
}

export function buildServiceSideImageSettings({
  settings,
  imageEnabled,
  profileId,
  apiType,
  model,
}: {
  settings: AppSettings;
  imageEnabled: boolean;
  profileId: string;
  apiType: string;
  model: string;
}): AppSettings {
  const result: AppSettings = { ...settings };
  const serviceSide = isRecord(settings.service_side)
    ? { ...settings.service_side }
    : {};
  serviceSide.image_input = {
    enabled: imageEnabled,
    profile_id: profileId.trim(),
    api_type: apiType.trim(),
    model: model.trim(),
  };
  result.service_side = serviceSide as AppSettings["service_side"];
  return result;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
