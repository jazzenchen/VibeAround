import { expect, test } from "bun:test";

import {
  CONNECTION_AGENTS,
  emptyConnectionDraft,
  resolveProfileConnection,
} from "../src/Launch/connections";
import type { ProfileConnections, ProfileSummary } from "../src/Launch/types";

const profile: ProfileSummary = {
  id: "profile-test",
  label: "Profile Test",
  provider: "custom",
  providerLabel: "Custom",
  providerIcon: null,
  authMode: "api_key",
  apiTypes: ["openai-chat"],
  launchTargets: [],
  apiTypeWarnings: {},
  apiTypeModels: { "openai-chat": "provider-default" },
  apiTypeModelOptions: {
    "openai-chat": [
      { id: "provider-default", label: null },
      { id: "provider-extra", label: null },
    ],
  },
  apiTypeHeaders: {},
};

test("connection drafts keep bridge models as the only stored model representation", () => {
  const connections: ProfileConnections = {
    [profile.id]: {
      codex: {
        selectedApiType: "openai-responses",
        bridge: {
          "openai-responses": {
            enabled: true,
            targetApiType: "openai-chat",
            models: [
              {
                upstreamModel: "provider-extra",
                fakeModelId: "gpt-5.5",
              },
            ],
          },
        },
      },
    },
  };
  const codex = CONNECTION_AGENTS.find((agent) => agent.id === "codex")!;

  const resolved = resolveProfileConnection(profile, connections, codex);
  expect(resolved.selected.upstreamModel).toBe("provider-extra");
  expect(resolved.selected.fakeModelId).toBe("gpt-5.5");

  const draft = emptyConnectionDraft(profile, connections);
  const bridge = draft.codex.bridge?.["openai-responses"]!;
  expect(bridge.models).toEqual([
    {
      upstreamModel: "provider-extra",
      fakeModelId: "gpt-5.5",
      capabilities: undefined,
    },
  ]);
  expect("upstreamModel" in bridge).toBe(false);
  expect("fakeModelId" in bridge).toBe(false);
});
