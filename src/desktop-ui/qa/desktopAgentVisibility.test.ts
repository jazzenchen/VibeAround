import { expect, test } from "bun:test";

import type {
  AgentSummary,
  DesktopAppDetectionFile,
} from "../src/Launch/api";
import {
  addableDesktopAgents,
  visibleLaunchAgents,
} from "../src/Launch/desktopAgentVisibility";

function agent(id: string, directOnly: boolean): AgentSummary {
  return {
    id,
    display_name: id,
    description: id,
    install_type: null,
    pty_command: id,
    direct_only: directOnly,
    acp_program: "",
    acp_args: [],
  };
}

const agents = [
  agent("codex", false),
  agent("codex-desktop", true),
  agent("claude-desktop", true),
];

const desktopApps: DesktopAppDetectionFile = {
  apps: {
    "codex-desktop": {
      installed: true,
      launchCommand: "open -b com.openai.codex",
    },
    "claude-desktop": {
      installed: false,
      launchCommand: "open -a Claude",
    },
  },
};

test("shows detected and manually configured desktop agents", () => {
  const visible = visibleLaunchAgents(
    agents,
    new Set(["codex"]),
    desktopApps,
    {
      "claude-desktop": {
        executable: {
          path: "/Applications/Claude.app",
          source: "manual",
          sourceLabel: "Manual",
          rank: 0,
        },
      },
    },
  );

  expect(visible.map(({ id }) => id)).toEqual([
    "codex",
    "codex-desktop",
    "claude-desktop",
  ]);
});

test("keeps undetected desktop agents hidden without a manual path", () => {
  const visible = visibleLaunchAgents(
    agents,
    new Set(["codex"]),
    desktopApps,
    {},
  );

  expect(visible.map(({ id }) => id)).toEqual(["codex", "codex-desktop"]);
});

test("only offers desktop agents that are not already visible", () => {
  const addable = addableDesktopAgents(agents, [
    agent("codex", false),
    agent("codex-desktop", true),
  ]);

  expect(addable.map(({ id }) => id)).toEqual(["claude-desktop"]);
});
