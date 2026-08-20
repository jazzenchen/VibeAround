import type { AgentInfo, ProfileLaunchOption } from "@va/client";

export const DIRECT_PROFILE_ID = "direct";

export function launchProfilesForAgent(
  profiles: ProfileLaunchOption[],
  agentId: string,
): ProfileLaunchOption[] {
  return profiles.filter((profile) =>
    profile.launch_targets.some((target) => target.id === agentId),
  );
}

export function profileIdForAgent(
  agent: AgentInfo | undefined,
  profiles: ProfileLaunchOption[],
  selectedProfileId?: string,
): string {
  const compatibleProfiles = agent
    ? launchProfilesForAgent(profiles, agent.id)
    : [];
  if (
    selectedProfileId &&
    selectedProfileId !== DIRECT_PROFILE_ID &&
    compatibleProfiles.some((profile) => profile.id === selectedProfileId)
  ) {
    return selectedProfileId;
  }
  if (agent?.requires_profile) {
    return compatibleProfiles[0]?.id ?? DIRECT_PROFILE_ID;
  }
  return DIRECT_PROFILE_ID;
}

export function launchSelectionIsValid(
  agent: AgentInfo | undefined,
  profileId?: string | null,
): boolean {
  return !agent?.requires_profile || Boolean(
    profileId && profileId !== DIRECT_PROFILE_ID,
  );
}

export function shouldApplySocketAgentSelection(
  source: "config" | "system",
  hasBoundThread: boolean,
  hasStoredSelection: boolean,
): boolean {
  return source === "system" || (!hasBoundThread && !hasStoredSelection);
}
