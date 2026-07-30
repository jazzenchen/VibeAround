import type {
  AgentSummary,
  DesktopAppDetectionFile,
} from "./api";
import type { AgentLaunchPreference } from "./types";

function hasConfiguredPath(
  preference: AgentLaunchPreference | undefined,
): boolean {
  return Boolean(
    preference?.executable?.path.trim() ||
      preference?.executablePath?.trim(),
  );
}

export function visibleLaunchAgents(
  agents: AgentSummary[],
  enabledAgentIds: ReadonlySet<string> | null,
  desktopApps: DesktopAppDetectionFile | null,
  agentPreferences: Record<string, AgentLaunchPreference>,
): AgentSummary[] {
  return agents.filter((agent) => {
    if (!agent.direct_only) {
      return enabledAgentIds ? enabledAgentIds.has(agent.id) : true;
    }
    return Boolean(
      desktopApps?.apps[agent.id]?.installed ||
        hasConfiguredPath(agentPreferences[agent.id]),
    );
  });
}
