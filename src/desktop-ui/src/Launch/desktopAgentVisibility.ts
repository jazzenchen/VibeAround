import type {
  AgentSummary,
  DesktopAppDetectionFile,
} from "./api";
import type { AgentLaunchPreference } from "./types";

function hasConfiguredPath(
  preference: AgentLaunchPreference | undefined,
): boolean {
  return Boolean(preference?.executable?.path.trim());
}

export function visibleLaunchAgents(
  agents: AgentSummary[],
  enabledAgentIds: ReadonlySet<string> | null,
  desktopApps: DesktopAppDetectionFile | null,
  agentPreferences: Record<string, AgentLaunchPreference>,
): AgentSummary[] {
  return agents.filter((agent) => {
    if (agent.built_in) {
      return true;
    }
    if (!agent.direct_only) {
      return enabledAgentIds ? enabledAgentIds.has(agent.id) : true;
    }
    return Boolean(
      desktopApps?.apps[agent.id]?.installed ||
        hasConfiguredPath(agentPreferences[agent.id]),
    );
  });
}

export function addableDesktopAgents(
  agents: AgentSummary[],
  visibleAgents: AgentSummary[],
): AgentSummary[] {
  const visibleIds = new Set(visibleAgents.map(({ id }) => id));
  return agents.filter(
    (agent) => agent.direct_only && !visibleIds.has(agent.id),
  );
}
