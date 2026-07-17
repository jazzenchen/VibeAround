import { describe, expect, test } from "bun:test";

import { createSettingsPatch } from "../src/lib/settingsPatch";

describe("createSettingsPatch", () => {
  test("emits only changed nested fields", () => {
    expect(
      createSettingsPatch(
        { api_bridge: { enabled: true, port: 9000 }, onboarded: true },
        { api_bridge: { enabled: true, port: 9001 }, onboarded: true },
      ),
    ).toEqual([{ op: "add", path: "/api_bridge/port", value: 9001 }]);
  });

  test("removes absent fields and replaces changed arrays", () => {
    expect(
      createSettingsPatch(
        { enabled_agents: ["codex"], legacy: true },
        { enabled_agents: ["codex", "claude"] },
      ),
    ).toEqual([
      {
        op: "add",
        path: "/enabled_agents",
        value: ["codex", "claude"],
      },
      { op: "remove", path: "/legacy" },
    ]);
  });

  test("distinguishes a null value from removing a field", () => {
    expect(
      createSettingsPatch(
        { retry_429: { max_retries: 10 }, legacy: true },
        { retry_429: { max_retries: null } },
      ),
    ).toEqual([
      {
        op: "add",
        path: "/retry_429/max_retries",
        value: null,
      },
      { op: "remove", path: "/legacy" },
    ]);
  });

  test("returns an empty patch for equal settings", () => {
    const settings = { launcher: { selected_agent: "codex" } };
    expect(createSettingsPatch(settings, structuredClone(settings))).toEqual([]);
  });

  test("escapes JSON Pointer path segments", () => {
    expect(createSettingsPatch({}, { "a/b~c": true })).toEqual([
      { op: "add", path: "/a~1b~0c", value: true },
    ]);
  });

  test("treats prototype names as ordinary JSON keys", () => {
    const base = JSON.parse(
      '{"constructor":true,"toString":"custom","__proto__":{"enabled":true}}',
    );

    expect(createSettingsPatch(base, {})).toEqual([
      { op: "remove", path: "/constructor" },
      { op: "remove", path: "/toString" },
      { op: "remove", path: "/__proto__" },
    ]);
  });

  test("includes pending edits when an autosave starts from persisted settings", () => {
    const persisted = {
      im: {
        order: ["slack", "telegram"],
        channels: { slack: { agent: "codex" } },
      },
    };
    const reorderedWithPendingEdit = {
      im: {
        order: ["telegram", "slack"],
        channels: { slack: { agent: "claude" } },
      },
    };

    expect(createSettingsPatch(persisted, reorderedWithPendingEdit)).toEqual([
      {
        op: "add",
        path: "/im/order",
        value: ["telegram", "slack"],
      },
      {
        op: "add",
        path: "/im/channels/slack/agent",
        value: "claude",
      },
    ]);
  });
});
