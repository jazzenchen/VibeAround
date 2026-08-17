import { describe, expect, test } from "bun:test";

import {
  buildServiceSideImageProfiles,
  buildServiceSideImageSettings,
} from "../src/Settings/serviceSideImage";
import type { ProfileSummary } from "../src/Launch/types";

function profile(): ProfileSummary {
  return {
    id: "custom-openai",
    label: "Custom OpenAI",
    provider: "custom",
    providerLabel: "Custom",
    providerIcon: null,
    authMode: "api_key",
    apiTypes: ["openai-chat", "openai-responses", "anthropic"],
    launchTargets: [],
    apiTypeModels: {},
    apiTypeHeaders: {},
    apiTypeModelOptions: {
      "openai-chat": [
        { id: "chat-vision", label: "Chat Vision", capabilities: { image_input: true } },
      ],
      "openai-responses": [
        { id: "responses-vision", label: "Responses Vision", capabilities: { image_input: true } },
      ],
      anthropic: [
        { id: "text-only", label: "Text only" },
      ],
    },
  };
}

describe("service-side image settings", () => {
  test("keeps image-capable models separate by API type", () => {
    const bindings = buildServiceSideImageProfiles([profile()]);

    expect(bindings.map(({ id, apiType, models }) => ({ id, apiType, models }))).toEqual([
      {
        id: "custom-openai",
        apiType: "openai-chat",
        models: ["chat-vision"],
      },
      {
        id: "custom-openai",
        apiType: "openai-responses",
        models: ["responses-vision"],
      },
    ]);
  });

  test("persists the selected API type", () => {
    const settings = buildServiceSideImageSettings({
      settings: {},
      imageEnabled: true,
      profileId: " custom-openai ",
      apiType: " openai-responses ",
      model: " gpt-vision ",
    });

    expect(settings.service_side?.image_input).toEqual({
      enabled: true,
      profile_id: "custom-openai",
      api_type: "openai-responses",
      model: "gpt-vision",
    });
  });
});
