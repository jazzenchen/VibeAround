import { expect, test } from "bun:test";

import {
  buildProfileCopyDraft,
  copyProfileLabel,
} from "../src/Launch/profileClone";
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
  expect(copyProfileLabel("DeepSeek", "复制")).toBe("DeepSeek 复制");
});

test("copyProfileLabel skips labels that already exist", () => {
  expect(
    copyProfileLabel("DeepSeek", "Copy", [
      "DeepSeek",
      "DeepSeek Copy",
      "DeepSeek Copy 2",
    ]),
  ).toBe("DeepSeek Copy 3");
  expect(
    copyProfileLabel("DeepSeek Copy", "Copy", ["DeepSeek Copy 2"]),
  ).toBe("DeepSeek Copy 3");
  expect(
    copyProfileLabel("DeepSeek", "Copy+", [
      "DeepSeek Copy+",
      "DeepSeek Copy+ 2",
    ]),
  ).toBe("DeepSeek Copy+ 3");
});

test("buildProfileCopyDraft clones editable profile fields without the id", () => {
  const draft = buildProfileCopyDraft(sourceProfile, "Copy", [
    sourceProfile.label,
    "bo/deepseek-v4-flash Copy",
  ]);

  expect(draft).toEqual({
    label: "bo/deepseek-v4-flash Copy 2",
    provider: sourceProfile.provider,
    auth_mode: sourceProfile.auth_mode,
    api_types: sourceProfile.api_types,
    credentials: sourceProfile.credentials,
    overrides: sourceProfile.overrides,
    provider_settings: sourceProfile.provider_settings,
  });
  expect("id" in draft).toBe(false);
  expect(draft).not.toBe(sourceProfile);
  expect(draft.api_types).not.toBe(sourceProfile.api_types);
  expect(draft.credentials).not.toBe(sourceProfile.credentials);
  expect(draft.overrides).not.toBe(sourceProfile.overrides);
  expect(draft.overrides["openai-responses"]).not.toBe(
    sourceProfile.overrides["openai-responses"],
  );
  expect(draft.provider_settings).not.toBe(sourceProfile.provider_settings);
  expect(draft.provider_settings?.deepseek).not.toBe(
    sourceProfile.provider_settings?.deepseek,
  );

  draft.overrides["openai-responses"]!.model = "changed";
  expect(sourceProfile.overrides["openai-responses"]?.model).toBe(
    "deepseek-v4-flash",
  );
});
