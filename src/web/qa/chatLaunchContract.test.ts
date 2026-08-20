import { expect, test } from "bun:test";

import type { AgentInfo, ProfileLaunchOption } from "@va/client";
import {
  DIRECT_PROFILE_ID,
  launchSelectionIsValid,
  profileIdForAgent,
  shouldApplySocketAgentSelection,
} from "../src/components/chat/chatLaunchContract";

const vaAgent: AgentInfo = {
  id: "va-agent",
  name: "VibeAround Agent",
  description: "Built-in VibeAround coding agent",
  requires_profile: true,
};

const codex: AgentInfo = {
  id: "codex",
  name: "Codex",
  description: "Codex CLI",
  requires_profile: false,
};

const profiles: ProfileLaunchOption[] = [
  {
    id: "profile-a",
    label: "Profile A",
    provider: "openai",
    launch_targets: [
      {
        id: "va-agent",
        label: "VibeAround Agent",
        api_type: "openai-responses",
      },
    ],
  },
];

test("required-profile agents select a compatible profile instead of Direct", () => {
  expect(profileIdForAgent(vaAgent, profiles, DIRECT_PROFILE_ID)).toBe("profile-a");
  expect(launchSelectionIsValid(vaAgent, DIRECT_PROFILE_ID)).toBe(false);
  expect(launchSelectionIsValid(vaAgent, "profile-a")).toBe(true);
});

test("ordinary agents can keep Direct", () => {
  expect(profileIdForAgent(codex, profiles, DIRECT_PROFILE_ID)).toBe(DIRECT_PROFILE_ID);
  expect(launchSelectionIsValid(codex, DIRECT_PROFILE_ID)).toBe(true);
});

test("socket defaults do not replace an explicit handoff binding", () => {
  expect(shouldApplySocketAgentSelection("config", true, false)).toBe(false);
  expect(shouldApplySocketAgentSelection("config", false, true)).toBe(false);
  expect(shouldApplySocketAgentSelection("config", false, false)).toBe(true);
  expect(shouldApplySocketAgentSelection("system", true, true)).toBe(true);
});
