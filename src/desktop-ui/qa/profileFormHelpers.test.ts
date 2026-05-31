import { expect, test } from "bun:test";

import {
  defaultAuthMode,
  overrideForEndpoint,
  pruneOverrides,
  requiresProfileModel,
  selectedEndpoint,
} from "../src/Launch/profileFormHelpers";
import type { CatalogEntry } from "../src/Launch/types";

const geminiProvider: CatalogEntry = {
  id: "gemini",
  label: "Google Gemini / Vertex AI",
  icon: null,
  homepage: null,
  endpoints: [
    {
      id: "gemini-api",
      label: "Gemini API",
      api_type: "gemini",
      default_base_url: "https://generativelanguage.googleapis.com",
      models: [{ id: "gemini-2.5-flash", label: "Gemini 2.5 Flash" }],
      auth_modes: [],
    },
    {
      id: "google-accounts",
      label: "Google accounts",
      api_type: "gemini",
      default_base_url: "https://cloudcode-pa.googleapis.com",
      append_v1_path: false,
      models: [{ id: "gemini-2.5-flash", label: "Gemini 2.5 Flash" }],
      auth_modes: [
        {
          mode: "google_oauth",
          label: "Use Google account",
          fields: [],
        },
      ],
    },
    {
      id: "gemini-api",
      label: "Gemini API",
      api_type: "openai-chat",
      default_base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
      models: [{ id: "gemini-2.5-flash", label: "Gemini 2.5 Flash" }],
      capabilities: { reasoning_effort: true },
      auth_modes: [],
    },
    {
      id: "vertex-openai-compatible",
      label: "Vertex AI",
      api_type: "openai-chat",
      default_base_url: "",
      models: [{ id: "google/gemini-2.5-flash", label: "Gemini 2.5 Flash" }],
      capabilities: { reasoning_effort: true },
      auth_modes: [],
    },
  ],
};

const azureProvider: CatalogEntry = {
  id: "azure",
  label: "Azure OpenAI",
  icon: null,
  homepage: null,
  endpoints: [
    {
      api_type: "openai-responses",
      default_base_url: "",
      models: [],
      capabilities: { reasoning_effort: true },
      auth_modes: [],
    },
  ],
};

test("catalog model endpoints do not create profile model defaults", () => {
  const endpoint = geminiProvider.endpoints.find(
    (candidate) => candidate.api_type === "openai-chat" && candidate.id === "gemini-api",
  )!;

  expect(
    overrideForEndpoint(endpoint, {
      model: "gemini-2.5-pro",
      reasoning_effort: "high",
    }),
  ).toEqual({
    endpoint_id: "gemini-api",
    base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
  });
});

test("saving a catalog-backed profile drops legacy model settings", () => {
  const overrides = {
    "openai-chat": {
      endpoint_id: "gemini-api",
      model: "gemini-2.5-pro",
      reasoning_effort: "high",
    },
  };

  expect(pruneOverrides(overrides, ["openai-chat"], geminiProvider)).toEqual({
    "openai-chat": {
      endpoint_id: "gemini-api",
    },
  });
});

test("legacy loaded profile overrides still select their endpoint", () => {
  const endpoint = selectedEndpoint(geminiProvider, "openai-chat", {
    "openai-chat": {
      endpoint_id: "vertex-openai-compatible",
      model: "google/gemini-2.5-pro",
      reasoning_effort: "high",
    },
  });

  expect(endpoint?.id).toBe("vertex-openai-compatible");
});

test("google account gemini endpoint defaults to oauth auth", () => {
  expect(
    defaultAuthMode(geminiProvider, ["gemini"], {
      gemini: {
        endpoint_id: "google-accounts",
      },
    }),
  ).toBe("google_oauth");
});

test("endpoints without catalog models keep required deployment names", () => {
  const endpoint = azureProvider.endpoints[0];

  expect(requiresProfileModel(azureProvider, endpoint)).toBe(true);
  expect(
    pruneOverrides(
      {
        "openai-responses": {
          base_url: "https://example.openai.azure.com/openai/v1",
          model: "prod-gpt-5",
          reasoning_effort: "high",
        },
      },
      ["openai-responses"],
      azureProvider,
    ),
  ).toEqual({
    "openai-responses": {
      base_url: "https://example.openai.azure.com/openai/v1",
      model: "prod-gpt-5",
    },
  });
});
