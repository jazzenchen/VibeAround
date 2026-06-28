import type {
  LaunchSessionInfo,
  ProfileLaunchOption,
  WorkspaceItem,
} from "@va/client";

const SESSION_KEY_SEPARATOR = "\u0000";

export const ALL_AGENTS_FILTER = "__all_agents__";

export interface ChatSessionWorkspaceGroup {
  workspace: WorkspaceItem;
  sessions: LaunchSessionInfo[];
}

export function chatSessionKey(
  session: Pick<
    LaunchSessionInfo,
    "agent_id" | "workspace" | "session_id" | "thread_id"
  >,
) {
  if (session.thread_id) return session.thread_id;
  return [
    session.agent_id,
    session.workspace,
    session.session_id,
  ].join(SESSION_KEY_SEPARATOR);
}

export function profileTargetsAgent(profile: ProfileLaunchOption, agentId: string) {
  return profile.launch_targets.some((target) => target.id === agentId);
}

export function sessionSyncScope(
  agents: string[],
  workspaces: WorkspaceItem[],
  showArchived: boolean,
) {
  return [
    agents.join(","),
    showArchived ? "archived" : "active",
    ...workspaces.map((workspace) => workspace.path),
  ].join(SESSION_KEY_SEPARATOR);
}

export function sameLaunchSession(a: LaunchSessionInfo, b: LaunchSessionInfo) {
  return (
    a.agent_id === b.agent_id &&
    a.session_id === b.session_id &&
    a.title === b.title &&
    a.workspace === b.workspace &&
    a.updated_at === b.updated_at &&
    a.short_id === b.short_id &&
    a.archived === b.archived &&
    Boolean(a.active) === Boolean(b.active) &&
    (a.host_agent_id ?? null) === (b.host_agent_id ?? null) &&
    (a.host_profile_id ?? null) === (b.host_profile_id ?? null) &&
    (a.host_profile_label ?? null) === (b.host_profile_label ?? null) &&
    (a.host_provider ?? null) === (b.host_provider ?? null) &&
    (a.host_provider_label ?? null) === (b.host_provider_label ?? null) &&
    (a.thread_id ?? null) === (b.thread_id ?? null)
  );
}

export function normalizeSessionGroups(
  groups: ChatSessionWorkspaceGroup[],
  workspaces: WorkspaceItem[],
): ChatSessionWorkspaceGroup[] {
  const workspaceByPath = new Map(
    workspaces.map((workspace) => [workspace.path, workspace]),
  );
  return groups
    .flatMap((group) => {
      const path = group.workspace?.path;
      if (typeof path !== "string") return [];
      const workspace = workspaceByPath.get(path) ?? group.workspace;
      const sessions = Array.isArray(group.sessions)
        ? group.sessions.filter(isLaunchSessionInfo)
        : [];
      return [{ workspace, sessions }];
    })
    .filter((group) =>
      workspaces.length === 0 ||
      workspaces.some((workspace) => workspace.path === group.workspace.path),
    );
}

export function mergeSessionGroupUpdates(
  currentGroups: ChatSessionWorkspaceGroup[],
  updatedGroups: ChatSessionWorkspaceGroup[],
  workspaces: WorkspaceItem[],
  updatedAgentIds: string[],
): ChatSessionWorkspaceGroup[] {
  const workspaceByPath = new Map(
    workspaces.map((workspace) => [workspace.path, workspace]),
  );
  const updatedAgents = new Set(updatedAgentIds);
  const groups = new Map<string, ChatSessionWorkspaceGroup>();
  for (const workspace of workspaces) {
    groups.set(workspace.path, { workspace, sessions: [] });
  }
  for (const group of currentGroups) {
    const workspace = workspaceByPath.get(group.workspace.path) ?? group.workspace;
    groups.set(workspace.path, { workspace, sessions: group.sessions });
  }

  for (const group of normalizeSessionGroups(updatedGroups, workspaces)) {
    const current = groups.get(group.workspace.path) ?? {
      workspace: workspaceByPath.get(group.workspace.path) ?? group.workspace,
      sessions: [],
    };
    const sessions = new Map<string, LaunchSessionInfo>();
    for (const session of current.sessions) {
      if (!updatedAgents.has(session.agent_id)) {
        sessions.set(chatSessionKey(session), session);
      }
    }
    for (const session of group.sessions) {
      const key = chatSessionKey(session);
      const existing = sessions.get(key);
      sessions.set(
        key,
        existing && sameLaunchSession(existing, session) ? existing : session,
      );
    }
    const nextSessions = Array.from(sessions.values());
    groups.set(group.workspace.path, {
      workspace: current.workspace,
      sessions:
        nextSessions.length === current.sessions.length &&
        nextSessions.every((session, index) => session === current.sessions[index])
          ? current.sessions
          : nextSessions,
    });
  }
  return Array.from(groups.values());
}

function isLaunchSessionInfo(value: unknown): value is LaunchSessionInfo {
  if (!value || typeof value !== "object") return false;
  const item = value as Partial<LaunchSessionInfo>;
  return (
    typeof item.agent_id === "string" &&
    typeof item.session_id === "string" &&
    typeof item.title === "string" &&
    typeof item.workspace === "string" &&
    typeof item.updated_at === "number" &&
    typeof item.short_id === "string" &&
    typeof item.archived === "boolean" &&
    (
      item.host_agent_id === undefined ||
      item.host_agent_id === null ||
      typeof item.host_agent_id === "string"
    ) &&
    (
      item.host_profile_id === undefined ||
      item.host_profile_id === null ||
      typeof item.host_profile_id === "string"
    ) &&
    (
      item.host_profile_label === undefined ||
      item.host_profile_label === null ||
      typeof item.host_profile_label === "string"
    ) &&
    (
      item.host_provider === undefined ||
      item.host_provider === null ||
      typeof item.host_provider === "string"
    ) &&
    (
      item.host_provider_label === undefined ||
      item.host_provider_label === null ||
      typeof item.host_provider_label === "string"
    ) &&
    (
      item.thread_id === undefined ||
      item.thread_id === null ||
      typeof item.thread_id === "string"
    )
  );
}
