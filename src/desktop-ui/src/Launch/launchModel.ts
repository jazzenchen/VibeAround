import type { useI18n } from "@va/i18n";

import type {
  LaunchSessionSummary,
  LauncherPreferences,
  TerminalOption,
  WorkspaceOption,
} from "./api";
import {
  CONNECTION_AGENTS,
  apiTypeProtocolLabel,
  apiTypeRouteLabel,
  resolveProfileConnection,
  type ConnectionAgentDef,
} from "./connections";
import type { ConnectionAgentId, ProfileSummary } from "./types";

const PROXY_AGENTS = new Set<string>([
  "claude",
  "claude-desktop",
  "codex",
  "codex-desktop",
  "gemini",
  "opencode",
  "pi",
]);
const SESSION_RESUME_AGENTS = new Set<string>([
  "claude",
  "codex",
  "pi",
  "cursor",
  "gemini",
  "opencode",
  "qwen-code",
]);

export type ExpandedBlock = "profile" | "workspace" | "session";
export type TranslateFn = ReturnType<typeof useI18n>["t"];
export type ProfileChoice =
  | { kind: "direct" }
  | { kind: "profile"; profileId: string };
export type SessionChoice = { kind: "session"; sessionId: string } | null;

export function moveItemBefore(items: string[], from: string, to: string): string[] {
  if (from === to) return items;
  const fromIndex = items.indexOf(from);
  const toIndex = items.indexOf(to);
  if (fromIndex < 0 || toIndex < 0) return items;
  const next = [...items];
  const [item] = next.splice(fromIndex, 1);
  next.splice(toIndex, 0, item);
  return next;
}

export function mergeOrderedSubset(
  allIds: string[],
  subsetIds: Set<string>,
  orderedSubsetIds: string[],
): string[] {
  const queue = [...orderedSubsetIds];
  return allIds.map((id) => (subsetIds.has(id) ? (queue.shift() ?? id) : id));
}

export function isSortableWorkspace(workspace: WorkspaceOption): boolean {
  return workspace.kind === "workspace" && !workspace.isDefault;
}

export function canDeleteWorkspace(workspace: WorkspaceOption): boolean {
  return workspace.kind === "workspace" && !workspace.isDefault;
}

export function currentWorkspace(prefs: LauncherPreferences | null): WorkspaceOption {
  if (!prefs) {
    return {
      path: "",
      label: "Workspace",
      detail: "",
      kind: "selected",
      isDefault: false,
    };
  }
  return (
    prefs.workspaceOptions.find((option) => option.path === prefs.workspace) ?? {
      path: prefs.workspace,
      label: shortPathLabel(prefs.workspace),
      detail: prefs.workspace,
      kind: "selected",
      isDefault: false,
    }
  );
}

export function currentTerminal(prefs: LauncherPreferences | null): TerminalOption | null {
  if (!prefs) return null;
  return (
    prefs.options.find((option) => option.id === prefs.terminal) ??
    prefs.options[0] ??
    null
  );
}

export function agentWorkspace(prefs: LauncherPreferences, agentId: string): string {
  if (!agentId) return prefs.workspace;
  return prefs.agentPreferences[agentId]?.workspace || prefs.workspace;
}

export function agentProfileId(
  prefs: LauncherPreferences,
  agentId: string,
): string | undefined {
  if (prefs.defaultAgent === agentId) {
    return prefs.defaultProfileId ?? undefined;
  }
  return (
    prefs.agentPreferences[agentId]?.profileId ??
    prefs.defaultProfiles[agentId] ??
    undefined
  );
}

export function isGlobalDefaultDirect(
  prefs: LauncherPreferences,
  agentId: string,
): boolean {
  return prefs.defaultAgent === agentId && !prefs.defaultProfileId;
}

export function isGlobalDefaultProfile(
  prefs: LauncherPreferences,
  agentId: string,
  profileId: string,
): boolean {
  return prefs.defaultAgent === agentId && prefs.defaultProfileId === profileId;
}

export function profileById(
  profiles: ProfileSummary[],
  profileId: string | undefined,
): ProfileSummary | null {
  if (!profileId) return null;
  return profiles.find((profile) => profile.id === profileId) ?? null;
}

export function profileSupportsAgent(
  profile: ProfileSummary | null,
  agentId: string,
  prefs: LauncherPreferences | null,
): boolean {
  if (!profile || !agentId) return false;
  if (!isBridgeAgent(agentId)) {
    return profile.launchTargets.some((target) => target.id === agentId);
  }
  if (!prefs) return false;
  const resolved = resolveProfileConnection(
    profile,
    prefs.profileConnections,
    agentConnectionDef(agentId),
  );
  return resolved.status !== "unsupported";
}

export function profileAvailability(
  profile: ProfileSummary,
  agentId: string,
  prefs: LauncherPreferences,
  t: TranslateFn,
): { launchable: boolean; reason?: string } {
  if (profileSupportsAgent(profile, agentId, prefs)) {
    return { launchable: true };
  }

  if (isBridgeAgent(agentId)) {
    const resolved = resolveProfileConnection(
      profile,
      prefs.profileConnections,
      agentConnectionDef(agentId),
    );
    if (resolved.targetOptions.length > 0) {
      return {
        launchable: false,
        reason: t('Enable API bridge for "{{profile}}" to launch {{agent}} with {{api}}', {
          profile: profile.label,
          agent: agentLabel(agentId),
          api: apiTypeProtocolDisplayLabel(resolved.selectedApiType),
        }),
      };
    }
  }

  return {
    launchable: false,
    reason: t('"{{profile}}" does not support {{agent}} yet', {
      profile: profile.label,
      agent: agentLabel(agentId),
    }),
  };
}

export function selectionUnavailableReason(
  choice: ProfileChoice,
  profile: ProfileSummary | null,
  agentId: string,
  prefs: LauncherPreferences,
  t: TranslateFn,
): string | undefined {
  if (choice.kind === "direct") return undefined;
  if (!profile) return t("Selected profile is missing");
  return profileAvailability(profile, agentId, prefs, t).reason;
}

export function profileSummary(
  profile: ProfileSummary,
  agentId: string,
  prefs: LauncherPreferences,
  t: TranslateFn,
) {
  if (isBridgeAgent(agentId)) {
    const resolved = resolveProfileConnection(
      profile,
      prefs.profileConnections,
      agentConnectionDef(agentId),
    );
    if (isDesktopBridgeAgent(agentId) && resolved.status !== "unsupported") {
      const bridgeEnabled = resolved.status === "via_bridge";
      const targetApiType =
        bridgeEnabled && resolved.selected.targetApiType
          ? resolved.selected.targetApiType
          : resolved.selectedApiType;
      return {
        title: profile.label,
        detail: profile.providerLabel,
        bridge: true,
        bridgeLabel: bridgeEnabled
          ? t("Desktop bridge")
          : t("Desktop bridge on launch"),
        route: t("{{agent}} → {{provider}} {{apiType}}", {
          agent: agentLabel(agentId),
          provider: profile.providerLabel,
          apiType: apiTypeProtocolDisplayLabel(targetApiType),
        }),
      };
    }
    if (resolved.status === "via_bridge" && resolved.selected.targetApiType) {
      return {
        title: profile.label,
        detail: profile.providerLabel,
        bridge: true,
        route: `${agentLabel(agentId)} ${apiTypeProtocolDisplayLabel(resolved.selectedApiType)} -> ${profile.providerLabel} ${apiTypeProtocolDisplayLabel(resolved.selected.targetApiType)}`,
      };
    }
    if (resolved.status === "native") {
      return {
        title: profile.label,
        detail: profile.providerLabel,
        bridge: false,
        route: `${profile.providerLabel} -> ${agentLabel(agentId)} ${apiTypeProtocolDisplayLabel(resolved.selectedApiType)}`,
      };
    }
    if (resolved.selected.targetApiType) {
      return {
        title: profile.label,
        detail: profile.providerLabel,
        bridge: false,
        route: t("{{clientApi}} -> {{targetApi}} (API bridge off)", {
          clientApi: apiTypeProtocolDisplayLabel(resolved.selectedApiType),
          targetApi: apiTypeRouteDisplayLabel(resolved.selected.targetApiType),
        }),
      };
    }
  }
  const target = profile.launchTargets.find((target) => target.id === agentId);
  return {
    title: profile.label,
    detail: profile.providerLabel,
    bridge: false,
    route: target
      ? `${profile.providerLabel} -> ${agentLabel(agentId)} ${target.apiType}`
      : profile.providerLabel,
  };
}

export function isSelectionLaunchable(
  choice: ProfileChoice,
  profile: ProfileSummary | null,
  agentId: string,
  prefs: LauncherPreferences,
): boolean {
  if (choice.kind === "direct") return true;
  return profileSupportsAgent(profile, agentId, prefs);
}

export function agentConnectionDef(agentId: string): ConnectionAgentDef {
  const id = connectionAgentId(agentId);
  return (
    CONNECTION_AGENTS.find((agent) => agent.id === id) ??
    CONNECTION_AGENTS.find((agent) => agent.id === "codex")!
  );
}

export function isBridgeAgent(agentId: string): boolean {
  return PROXY_AGENTS.has(agentId);
}

export function isDesktopBridgeAgent(agentId: string): boolean {
  return agentId === "claude-desktop" || agentId === "codex-desktop";
}

export function connectionAgentId(agentId: string): ConnectionAgentId | null {
  return PROXY_AGENTS.has(agentId) ? (agentId as ConnectionAgentId) : null;
}

export function agentSupportsSessionResume(agentId: string): boolean {
  return SESSION_RESUME_AGENTS.has(agentId);
}

export function resolveSelectedSession(
  choice: SessionChoice,
  sessions: LaunchSessionSummary[],
): LaunchSessionSummary | null {
  if (choice?.kind === "session") {
    return sessions.find((session) => session.sessionId === choice.sessionId) ?? null;
  }
  return null;
}

export function apiTypeProtocolDisplayLabel(apiType: string): string {
  return apiTypeProtocolLabel(apiType);
}

export function apiTypeRouteDisplayLabel(apiType: string): string {
  return apiTypeRouteLabel(apiType);
}

export function relativeTime(updatedAt: number, t: TranslateFn): string {
  if (!updatedAt) return "-";
  const diff = Math.max(0, Math.floor(Date.now() / 1000) - updatedAt);
  if (diff < 60) return t("just now");
  if (diff < 3600) {
    return t("{{count}} min ago", { count: Math.floor(diff / 60) });
  }
  if (diff < 86400) {
    return t("{{count}} h ago", { count: Math.floor(diff / 3600) });
  }
  if (diff < 604800) {
    return t("{{count}} d ago", { count: Math.floor(diff / 86400) });
  }
  return new Date(updatedAt * 1000).toLocaleDateString();
}

export function shortPathLabel(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts.at(-1) ?? path;
}

export function agentLabel(agentId: string): string {
  switch (agentId) {
    case "claude":
      return "Claude";
    case "claude-desktop":
      return "Claude Desktop";
    case "codex":
      return "Codex";
    case "codex-desktop":
      return "ChatGPT Desktop (Codex)";
    case "pi":
      return "Pi";
    case "gemini":
      return "Gemini";
    case "cursor":
      return "Cursor";
    case "kiro":
      return "Kiro";
    case "qwen-code":
      return "Qwen";
    case "opencode":
      return "OpenCode";
    default:
      return agentId;
  }
}
