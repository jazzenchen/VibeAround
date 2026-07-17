import { describe, expect, test } from "bun:test";

import { createSettingsPatch } from "../src/lib/settingsPatch";

describe("createSettingsPatch", () => {
  test("emits only changed nested fields", () => {
    expect(
      createSettingsPatch(
        { api_bridge: { enabled: true, port: 9000 }, onboarded: true },
        { api_bridge: { enabled: true, port: 9001 }, onboarded: true },
      ),
    ).toEqual({ api_bridge: { port: 9001 } });
  });

  test("uses null for removals and replaces changed arrays", () => {
    expect(
      createSettingsPatch(
        { enabled_agents: ["codex"], legacy: true },
        { enabled_agents: ["codex", "claude"] },
      ),
    ).toEqual({
      enabled_agents: ["codex", "claude"],
      legacy: null,
    });
  });

  test("returns an empty patch for equal settings", () => {
    const settings = { launcher: { selected_agent: "codex" } };
    expect(createSettingsPatch(settings, structuredClone(settings))).toEqual({});
  });
});
