import { expect, test } from "bun:test";

import { buildProfileCopyDraft, copyProfileLabel } from "../src/Launch/profileClone";
import type { ProfileDef } from "../src/Launch/types";

const sourceProfile: ProfileDef = {
  id: "profile-1",
  label: "bo/deepseek-v4-flash",
  provider: "deepseek",
  auth_mode: "api_key",
  api_types: ["openai-responses", "openai-chat"],
  credentials: {
    api_key: "secret",
  },
  overrides: {
    "openai-responses": {
      endpoint_id: "responses",
      model: "deepseek-v4-flash",
      base_url: "https://api.example.test/responses",
    },
  },
  provider_settings: {
    deepseek: {
      thinking: true,
      replay_reasoning_content: true,
    },
  },
};

test("copyProfileLabel appends a localized copy suffix", () => {
  expect(copyProfileLabel("DeepSeek", "Copy")).toBe("DeepSeek Copy");
  expect(copyProfileLabel("DeepSeek Copy", "Copy")).toBe("DeepSeek Copy 2");
  expect(copyProfileLabel("DeepSeek Copy 2", "Copy")).toBe("DeepSeek Copy 3");
});

test("buildProfileCopyDraft clones editable profile fields without the id", () => {
  const draft = buildProfileCopyDraft(sourceProfile, "Copy");

  expect(draft).toEqual({
    label: "bo/deepseek-v4-flash Copy",
    provider: sourceProfile.provider,
    auth_mode: sourceProfile.auth_mode,
    api_types: sourceProfile.api_types,
    credentials: sourceProfile.credentials,
    overrides: sourceProfile.overrides,
    provider_settings: sourceProfile.provider_settings,
  });
  expect("id" in draft).toBe(false);
  expect(draft).not.toBe(sourceProfile);
});
