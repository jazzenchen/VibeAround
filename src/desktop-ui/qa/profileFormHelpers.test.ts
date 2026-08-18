import { expect, test } from "bun:test";

import {
  apiConfigForEndpoint,
  apiConfigsForEndpoints,
  defaultAuthMode,
  requiresProfileModel,
  selectedEndpoint,
  shouldShowBaseUrl,
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

const mimoProvider: CatalogEntry = {
  id: "mimo",
  label: "Xiaomi MiMo",
  icon: null,
  homepage: null,
  endpoints: [
    {
      id: "pay-as-you-go",
      label: "Pay-as-you-go",
      api_type: "openai-chat",
      default_base_url: "https://api.xiaomimimo.com/v1",
      append_v1_path: false,
      models: [{ id: "mimo-v2.5-pro", label: "MiMo V2.5 Pro" }],
      auth_modes: [],
    },
    {
      id: "token-plan-cn",
      label: "Token Plan CN",
      api_type: "openai-chat",
      default_base_url: "https://token-plan-cn.xiaomimimo.com/v1",
      append_v1_path: false,
      models: [
        { id: "mimo-v2.5-pro", label: "MiMo V2.5 Pro" },
        { id: "mimo-v2.5", label: "MiMo V2.5" },
      ],
      auth_modes: [],
    },
  ],
};

test("catalog endpoint materializes one canonical API config", () => {
  const endpoint = geminiProvider.endpoints.find(
    (candidate) => candidate.api_type === "openai-chat" && candidate.id === "gemini-api",
  )!;

  const config = apiConfigForEndpoint(endpoint, {
    model: "gemini-2.5-pro",
    reasoning_effort: "high",
  });
  expect(config.enabled).toBe(true);
  expect(config.endpoint_id).toBe("gemini-api");
  expect(config.base_url).toBe(
    "https://generativelanguage.googleapis.com/v1beta/openai",
  );
  expect(config.model).toBe("gemini-2.5-pro");
  expect(config.reasoning_effort).toBe("high");
});

test("mimo token plan uses catalog default model without profile override", () => {
  const endpoint = mimoProvider.endpoints.find(
    (candidate) => candidate.id === "token-plan-cn",
  )!;

  expect(requiresProfileModel(mimoProvider, endpoint)).toBe(false);
  expect(shouldShowBaseUrl(mimoProvider, endpoint, {})).toBe(false);
  expect(
    shouldShowBaseUrl(mimoProvider, endpoint, {
      base_url: "https://token-plan-cn.xiaomimimo.com/v1",
    }),
  ).toBe(false);
  expect(
    shouldShowBaseUrl(mimoProvider, endpoint, {
      base_url: "https://example.test/v1",
    }),
  ).toBe(true);
  expect(
    apiConfigForEndpoint(endpoint, {
      model: "mimo-v2.5-pro",
    }).model,
  ).toBe("mimo-v2.5-pro");
});

test("canonical API config selects its endpoint", () => {
  const endpoint = selectedEndpoint(geminiProvider, "openai-chat", {
    "openai-chat": {
      endpoint_id: "vertex-openai-compatible",
      model: "google/gemini-2.5-pro",
      reasoning_effort: "high",
    },
  });

  expect(endpoint?.id).toBe("vertex-openai-compatible");
});

test("switching endpoints rewrites endpoint-owned defaults", () => {
  const vertex = geminiProvider.endpoints.find(
    (candidate) => candidate.id === "vertex-openai-compatible",
  )!;
  const configs = apiConfigsForEndpoints([vertex], {
    "openai-chat": {
      enabled: true,
      endpoint_id: "gemini-api",
      base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
      model: "gemini-2.5-flash",
      headers: [{ name: "X-Test", value: "keep-me" }],
    },
  });

  expect(configs["openai-chat"]?.endpoint_id).toBe(
    "vertex-openai-compatible",
  );
  expect(configs["openai-chat"]?.base_url).toBeUndefined();
  expect(configs["openai-chat"]?.model).toBe("google/gemini-2.5-flash");
  expect(configs["openai-chat"]?.headers).toEqual([
    { name: "X-Test", value: "keep-me" },
  ]);
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
  const config = apiConfigForEndpoint(endpoint, {
    base_url: "https://example.openai.azure.com/openai/v1",
    model: "prod-gpt-5",
    reasoning_effort: "high",
  });
  expect(config.base_url).toBe("https://example.openai.azure.com/openai/v1");
  expect(config.model).toBe("prod-gpt-5");
  expect(config.reasoning_effort).toBe("high");
});
