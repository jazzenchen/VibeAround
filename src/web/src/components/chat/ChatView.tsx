"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  archiveLaunchSession,
  createWorkspace,
  getLaunchSessionsBatch,
  getProfiles,
  getWorkspaces,
} from "@/api/sessions";
import { getAgentDisplayName } from "@/lib/agents";
import type { ChatRuntimeStatus } from "@/lib/dashboard-types";
import type {
  LaunchSessionInfo,
  ProfileLaunchOption,
  WebVerboseSettings,
  WorkspaceItem,
} from "@va/client";
import { useI18n } from "@va/i18n";
import { ChatInput } from "./ChatInput";
import { ChatHeader } from "./ChatHeader";
import { ChatRuntimeHost } from "./ChatRuntimeHost";
import {
  chatRuntimeKeyForSession,
  createDraftRuntimeKey,
  INITIAL_RUNTIME_KEY,
} from "./chatRuntimeKeys";
import type {
  ChatRuntimeActions,
  ChatRuntimeSnapshot,
  ChatRuntimeSpec,
} from "./chatRuntimeTypes";
import { EMPTY_RUNTIME_SNAPSHOT } from "./chatRuntimeTypes";
import { deleteCachedChatSession } from "./chatSessionCache";
import {
  chatSessionKey,
  ALL_AGENTS_FILTER,
  mergeSessionGroupUpdates,
  profileTargetsAgent,
  sessionSyncScope,
  type ChatSessionWorkspaceGroup,
} from "./chatSessionModel";
import { ChatSessionSidebar } from "./ChatSessionSidebar";
import {
  clampSessionSidebarWidth,
  clearStoredActiveLaunchSession,
  readCachedLaunchSessionGroups,
  readStoredActiveLaunchSession,
  readStoredLaunchSelection,
  readStoredSessionSidebarWidth,
  storedActiveLaunchSessionFromInfo,
  storedActiveLaunchSessionToInfo,
  writeCachedLaunchSessionGroups,
  writeStoredActiveLaunchSession,
  writeStoredLaunchSelection,
  writeStoredSessionSidebarWidth,
} from "./chatSessionStorage";
import { shortSessionId } from "./chatSessionDisplay";
import { ChatMessageList } from "./ChatMessageList";
import { NewChatAgentPicker } from "./NewChatAgentPicker";
import { NewChatHome } from "./NewChatHome";
import { NewChatWorkspacePicker } from "./NewChatWorkspacePicker";
import { PendingPermissions } from "./PendingPermissions";
import { SubagentPanel } from "./SubagentPanel";
import { currentUnixSeconds } from "./chatTime";
import type { ChatSessionSelection } from "./chatTypes";
import { useChatAttachments } from "./useChatAttachments";

interface ChatViewProps {
  webSettings: WebVerboseSettings;
  onStatusChange?: (status: ChatRuntimeStatus) => void;
  onOpenAppSidebar?: () => void;
}

const DIRECT_PROFILE_ID = "direct";

export function ChatView({
  webSettings,
  onStatusChange,
  onOpenAppSidebar,
}: ChatViewProps) {
  const { t } = useI18n();
  const [storedLaunchSelection] = useState(readStoredLaunchSelection);
  const [input, setInput] = useState("");
  const {
    attachments,
    attachmentsUploading,
    attachmentsUploadingCount,
    attachmentError,
    clearAttachments,
    handleFilesSelected,
    handleRemoveAttachment,
  } = useChatAttachments(t);
  const [selectedAgent, setSelectedAgent] = useState<string>(
    storedLaunchSelection.agentId ?? "claude",
  );
  const [sidebarAgentFilter, setSidebarAgentFilter] = useState<string>(ALL_AGENTS_FILTER);
  const [profiles, setProfiles] = useState<ProfileLaunchOption[]>([]);
  const [profileSelections, setProfileSelections] = useState<Record<string, string | undefined>>(
    () =>
      storedLaunchSelection.agentId
        ? {
            [storedLaunchSelection.agentId]:
              storedLaunchSelection.profileId ?? DIRECT_PROFILE_ID,
          }
        : {},
  );
  const [syncedLaunchSessionGroups, setSyncedLaunchSessionGroups] = useState<
    ChatSessionWorkspaceGroup[]
  >([]);
  const [workspaces, setWorkspaces] = useState<WorkspaceItem[]>([]);
  const [defaultWorkspacePath, setDefaultWorkspacePath] = useState<string | undefined>();
  const [selectedWorkspacePath, setSelectedWorkspacePath] = useState<string | undefined>();
  const [workspacesLoading, setWorkspacesLoading] = useState(false);
  const [workspaceCreating, setWorkspaceCreating] = useState(false);
  const [workspaceCreateError, setWorkspaceCreateError] = useState<string | undefined>();
  const [sessionsLoading, setSessionsLoading] = useState(false);
  const [sessionSelections, setSessionSelections] = useState<Record<string, ChatSessionSelection>>(
    {},
  );
  const [selectedLaunchSessions, setSelectedLaunchSessions] = useState<
    Record<string, LaunchSessionInfo | undefined>
  >({});
  const [archivingSessionId, setArchivingSessionId] = useState<string | undefined>();
  const [showSessionSidebar, setShowSessionSidebar] = useState(true);
  const [sessionSidebarWidth, setSessionSidebarWidth] = useState(
    readStoredSessionSidebarWidth,
  );
  const [mobileSessionSidebarOpen, setMobileSessionSidebarOpen] = useState(false);
  const syncedSessionScopeRef = useRef<string | undefined>(undefined);
  const syncedLaunchSessionAgentsRef = useRef<Set<string>>(new Set());
  const sessionSyncRequestIdRef = useRef(0);
  const restoredActiveLaunchSessionRef = useRef(false);
  const storedActiveLaunchSessionKeyRef = useRef<string | undefined>(undefined);
  const [runtimeKeys, setRuntimeKeys] = useState<string[]>([INITIAL_RUNTIME_KEY]);
  const [activeRuntimeKey, setActiveRuntimeKey] = useState(INITIAL_RUNTIME_KEY);
  const activeRuntimeKeyRef = useRef(activeRuntimeKey);
  const [runtimeSpecs, setRuntimeSpecs] = useState<Record<string, ChatRuntimeSpec>>(
    () => ({
      [INITIAL_RUNTIME_KEY]: {
        agentId: storedLaunchSelection.agentId ?? "claude",
        profileId: storedLaunchSelection.profileId ?? DIRECT_PROFILE_ID,
      },
    }),
  );
  const [runtimeSnapshots, setRuntimeSnapshots] = useState<
    Record<string, ChatRuntimeSnapshot>
  >({});
  const [subagentPanelOpen, setSubagentPanelOpen] = useState(true);
  const [selectedSubagentId, setSelectedSubagentId] = useState<string | undefined>();
  const runtimeActionsRef = useRef<Record<string, ChatRuntimeActions>>({});
  const syncedPromptDoneRef = useRef<Record<string, number>>({});
  const syncedActiveSessionRef = useRef<Record<string, string | undefined>>({});

  useEffect(() => {
    activeRuntimeKeyRef.current = activeRuntimeKey;
  }, [activeRuntimeKey]);

  const handleSocketAgentSelected = useCallback(
    (runtimeKey: string, agentId: string, source: "config" | "system") => {
      if (runtimeKey !== activeRuntimeKeyRef.current) return;
      if (source === "config" && storedLaunchSelection.agentId) return;
      setSelectedAgent(agentId);
    },
    [storedLaunchSelection.agentId],
  );

  const handleRuntimeSnapshot = useCallback(
    (runtimeKey: string, snapshot: ChatRuntimeSnapshot) => {
      setRuntimeSnapshots((prev) => ({ ...prev, [runtimeKey]: snapshot }));
    },
    [],
  );

  const handleRuntimeActions = useCallback(
    (runtimeKey: string, actions: ChatRuntimeActions | null) => {
      if (actions) {
        runtimeActionsRef.current[runtimeKey] = actions;
        return;
      }
      delete runtimeActionsRef.current[runtimeKey];
    },
    [],
  );

  useEffect(() => {
    const staleLaunches = Object.entries(runtimeSpecs)
      .map(([runtimeKey, spec]) => {
        const sessionId = runtimeSnapshots[runtimeKey]?.meta.sessionId;
        const launchSession = spec.launchSession;
        if (!sessionId || !launchSession || launchSession.session_id === sessionId) {
          return null;
        }
        return {
          agentId: spec.agentId,
          runtimeKey,
          sessionId: launchSession.session_id,
        };
      })
      .filter(
        (
          item,
        ): item is { agentId: string; runtimeKey: string; sessionId: string } =>
          item !== null,
      );
    if (staleLaunches.length === 0) return;

    setRuntimeSpecs((prev) => {
      let changed = false;
      const next = { ...prev };
      for (const stale of staleLaunches) {
        const spec = next[stale.runtimeKey];
        if (!spec || spec.launchSession?.session_id !== stale.sessionId) continue;
        next[stale.runtimeKey] = { ...spec, launchSession: undefined };
        changed = true;
      }
      return changed ? next : prev;
    });
    setSelectedLaunchSessions((prev) => {
      let changed = false;
      const next = { ...prev };
      for (const stale of staleLaunches) {
        if (next[stale.agentId]?.session_id !== stale.sessionId) continue;
        delete next[stale.agentId];
        changed = true;
      }
      return changed ? next : prev;
    });
    setSessionSelections((prev) => {
      let changed = false;
      const next = { ...prev };
      for (const stale of staleLaunches) {
        const selection = next[stale.agentId];
        if (selection?.kind !== "resume" || selection.sessionId !== stale.sessionId) continue;
        next[stale.agentId] = { kind: "current" };
        changed = true;
      }
      return changed ? next : prev;
    });
  }, [runtimeSnapshots, runtimeSpecs]);

  const activeRuntime = runtimeSnapshots[activeRuntimeKey] ?? EMPTY_RUNTIME_SNAPSHOT;
  const activeRuntimeActions = runtimeActionsRef.current[activeRuntimeKey];
  const messages = activeRuntime.messages;
  const connected = activeRuntime.connected;
  const streaming = activeRuntime.streaming;
  const meta = activeRuntime.meta;
  const agents = useMemo(
    () =>
      Object.values(runtimeSnapshots).find((snapshot) => snapshot.agents.length > 0)
        ?.agents ?? [],
    [runtimeSnapshots],
  );
  const pendingPermissions = activeRuntime.pendingPermissions;
  const sessionMode = activeRuntime.sessionMode;
  const resumeReplay = activeRuntime.resumeReplay;
  const multiAgentTurns = activeRuntime.multiAgentTurns;
  const subagents = activeRuntime.subagents;
  const replayBlocksInput = Boolean(
    resumeReplay && resumeReplay.blocking !== false,
  );
  const sendMessage = activeRuntimeActions?.sendMessage;
  const stopStreaming = activeRuntimeActions?.stopStreaming;
  const setSessionMode = activeRuntimeActions?.setSessionMode;
  const setSessionConfigOption = activeRuntimeActions?.setSessionConfigOption;
  const sendPermissionResponse = activeRuntimeActions?.sendPermissionResponse;
  const cancelPermissionRequest = activeRuntimeActions?.cancelPermissionRequest;

  useEffect(() => {
    if (subagents.length === 0) {
      if (selectedSubagentId) setSelectedSubagentId(undefined);
      return;
    }
    if (!selectedSubagentId || !subagents.some((agent) => agent.id === selectedSubagentId)) {
      setSelectedSubagentId(subagents[0].id);
    }
  }, [selectedSubagentId, subagents]);

  const launchSessionGroups = useMemo(
    () =>
      syncedLaunchSessionGroups.map((group) => ({
        ...group,
        sessions:
          sidebarAgentFilter === ALL_AGENTS_FILTER
            ? group.sessions
            : group.sessions.filter((session) => session.agent_id === sidebarAgentFilter),
      })),
    [sidebarAgentFilter, syncedLaunchSessionGroups],
  );
  const launchSessions = useMemo(
    () => syncedLaunchSessionGroups.flatMap((group) => group.sessions),
    [syncedLaunchSessionGroups],
  );
  const selectedAgentInfo = agents.find((agent) => agent.id === selectedAgent);
  const agentLabel = selectedAgentInfo?.name ?? getAgentDisplayName(selectedAgent);
  const selectedProfileId = profileSelections[selectedAgent] ?? DIRECT_PROFILE_ID;
  const selectedProfile = profiles.find((profile) => profile.id === selectedProfileId);
  const selectedWorkspace = workspaces.find(
    (workspace) => workspace.path === selectedWorkspacePath,
  );
  const activeWorkspacePath =
    runtimeSpecs[activeRuntimeKey]?.launchSession?.workspace ??
    runtimeSpecs[activeRuntimeKey]?.workspacePath ??
    resumeReplay?.workspace ??
    selectedWorkspace?.path ??
    defaultWorkspacePath;
  const sessionSelection = sessionSelections[selectedAgent] ?? { kind: "new" };
  const selectedLaunchSession =
    sessionSelection.kind === "resume" &&
    selectedLaunchSessions[selectedAgent]?.agent_id === selectedAgent &&
    selectedLaunchSessions[selectedAgent]?.session_id === sessionSelection.sessionId
      ? selectedLaunchSessions[selectedAgent]
      : sessionSelection.kind === "resume"
        ? launchSessions.find(
            (session) =>
              session.agent_id === selectedAgent &&
              session.session_id === sessionSelection.sessionId,
          )
        : undefined;
  const replayLoading = Boolean(resumeReplay);
  const chatStatus: ChatRuntimeStatus =
    pendingPermissions.length > 0
      ? "attention"
      : streaming || replayLoading
        ? "working"
        : connected
          ? "ready"
          : "connecting";
  const statusLabel =
    chatStatus === "attention"
      ? t("Agent needs input")
      : chatStatus === "working"
        ? replayLoading
          ? t("Loading chat history")
          : t("Agent working")
        : chatStatus === "ready"
          ? t("Local agent ready")
          : t("Connecting to local agent");
  const headerSessionLabel =
    sessionSelection.kind === "new"
      ? null
      : selectedLaunchSession
        ? selectedLaunchSession.title
        : meta.sessionId
          ? t("Current session")
          : null;
  const routeLabel =
    selectedProfileId && selectedProfile
      ? t("{{agent}} / {{profile}}", {
          agent: agentLabel,
          profile: selectedProfile.label,
        })
      : agentLabel;
  const showNewChatHome = messages.length === 0 && sessionSelection.kind !== "resume";
  const sidebarSessionsLoading = workspacesLoading || sessionsLoading;
  const displaySettings = useMemo(
    () => ({
      showThinking: webSettings.show_thinking,
      showTools: webSettings.show_tool_use,
    }),
    [webSettings],
  );
  const runtimeLaunchSessions = useMemo(() => {
    return Object.entries(runtimeSpecs).flatMap(([runtimeKey, spec]) => {
      const snapshot = runtimeSnapshots[runtimeKey];
      if (!snapshot || (!snapshot.streaming && !snapshot.resumeReplay)) return [];
      const sessionId = spec.launchSession?.session_id ?? snapshot.meta.sessionId;
      const workspacePath =
        spec.launchSession?.workspace ??
        spec.workspacePath ??
        selectedWorkspace?.path ??
        defaultWorkspacePath;
      if (!sessionId || !workspacePath) return [];
      const title =
        spec.launchSession?.title ??
        spec.title ??
        (snapshot.resumeReplay?.title || t("Current session"));
      return [
        {
          agent_id: spec.launchSession?.agent_id ?? spec.agentId,
          session_id: sessionId,
          title,
          workspace: workspacePath,
          updated_at: Math.max(
            spec.lastPromptAt ?? 0,
            spec.launchSession?.updated_at ?? 0,
            snapshot.resumeReplay?.updatedAt ?? 0,
          ),
          short_id: spec.launchSession?.short_id ?? shortSessionId(sessionId),
          archived: false,
          active: true,
        } satisfies LaunchSessionInfo,
      ];
    });
  }, [
    defaultWorkspacePath,
    runtimeSnapshots,
    runtimeSpecs,
    selectedWorkspace?.path,
    t,
  ]);
  const visibleRuntimeLaunchSessions = useMemo(
    () =>
      sidebarAgentFilter === ALL_AGENTS_FILTER
        ? runtimeLaunchSessions
        : runtimeLaunchSessions.filter(
            (session) => session.agent_id === sidebarAgentFilter,
          ),
    [runtimeLaunchSessions, sidebarAgentFilter],
  );
  const activeLaunchSessionKeys = useMemo(
    () =>
      new Set(
        launchSessionGroups
          .flatMap((group) => group.sessions)
          .filter((session) => session.active)
          .map((session) => chatSessionKey(session)),
      ),
    [launchSessionGroups],
  );
  const runtimeBusySessionKeys = useMemo(() => {
    const keys = new Set(activeLaunchSessionKeys);
    for (const session of visibleRuntimeLaunchSessions) {
      keys.add(chatSessionKey(session));
    }
    return keys;
  }, [activeLaunchSessionKeys, visibleRuntimeLaunchSessions]);
  const displayLaunchSessionGroups = useMemo(() => {
    if (visibleRuntimeLaunchSessions.length === 0) return launchSessionGroups;
    const groupsByWorkspace = new Map<string, ChatSessionWorkspaceGroup>();
    for (const group of launchSessionGroups) {
      groupsByWorkspace.set(group.workspace.path, {
        workspace: group.workspace,
        sessions: [...group.sessions],
      });
    }
    for (const session of visibleRuntimeLaunchSessions) {
      const workspace =
        groupsByWorkspace.get(session.workspace)?.workspace ??
        workspaces.find((item) => item.path === session.workspace) ?? {
          path: session.workspace,
          is_default: session.workspace === defaultWorkspacePath,
          is_builtin: false,
        };
      const group = groupsByWorkspace.get(session.workspace) ?? {
        workspace,
        sessions: [],
      };
      const existingIndex = group.sessions.findIndex(
        (item) => chatSessionKey(item) === chatSessionKey(session),
      );
      if (existingIndex >= 0) {
        group.sessions[existingIndex] = {
          ...group.sessions[existingIndex],
          ...session,
        };
      } else {
        group.sessions.unshift(session);
      }
      groupsByWorkspace.set(session.workspace, group);
    }
    return Array.from(groupsByWorkspace.values());
  }, [
    defaultWorkspacePath,
    launchSessionGroups,
    visibleRuntimeLaunchSessions,
    workspaces,
  ]);

  const createDraftRuntime = useCallback((agentId: string, workspacePath?: string) => {
    clearStoredActiveLaunchSession();
    storedActiveLaunchSessionKeyRef.current = undefined;
    const runtimeKey = createDraftRuntimeKey(agentId);
    setRuntimeSpecs((prev) => ({
      ...prev,
      [runtimeKey]: {
        agentId,
        profileId: profileSelections[agentId] ?? DIRECT_PROFILE_ID,
        workspacePath,
      },
    }));
    setRuntimeKeys((prev) => [...prev, runtimeKey]);
    setActiveRuntimeKey(runtimeKey);
    setSelectedAgent(agentId);
    if (workspacePath) setSelectedWorkspacePath(workspacePath);
    return runtimeKey;
  }, [profileSelections]);

  const activateRuntimeForSession = useCallback(
    (session: LaunchSessionInfo) => {
      const existingRuntime = Object.entries(runtimeSpecs).find(([runtimeKey, spec]) => {
        const snapshot = runtimeSnapshots[runtimeKey];
        const sessionId = spec.launchSession?.session_id ?? snapshot?.meta.sessionId;
        const workspace =
          spec.launchSession?.workspace ??
          spec.workspacePath ??
          snapshot?.resumeReplay?.workspace;
        return (
          spec.agentId === session.agent_id &&
          sessionId === session.session_id &&
          workspace === session.workspace
        );
      });
      if (existingRuntime) {
        const [runtimeKey] = existingRuntime;
        setRuntimeSpecs((prev) => ({
          ...prev,
          [runtimeKey]: {
            ...(prev[runtimeKey] ?? {
              agentId: session.agent_id,
              profileId: profileSelections[session.agent_id] ?? DIRECT_PROFILE_ID,
            }),
            agentId: session.agent_id,
            profileId:
              prev[runtimeKey]?.profileId ??
              profileSelections[session.agent_id] ??
              DIRECT_PROFILE_ID,
            workspacePath: session.workspace,
            launchSession: session,
            title: session.title,
          },
        }));
        setActiveRuntimeKey(runtimeKey);
        setSelectedAgent(session.agent_id);
        setSelectedWorkspacePath(session.workspace);
        return runtimeKey;
      }

      const runtimeKey = chatRuntimeKeyForSession(session);
      setRuntimeSpecs((prev) =>
        prev[runtimeKey]
          ? prev
          : {
              ...prev,
              [runtimeKey]: {
                agentId: session.agent_id,
                profileId: profileSelections[session.agent_id] ?? DIRECT_PROFILE_ID,
                workspacePath: session.workspace,
                launchSession: session,
                title: session.title,
                initialResume: {
                  agentId: session.agent_id,
                  profileId: profileSelections[session.agent_id] ?? DIRECT_PROFILE_ID,
                  launchSession: session,
                },
              },
            },
      );
      setRuntimeKeys((prev) =>
        prev.includes(runtimeKey) ? prev : [...prev, runtimeKey],
      );
      setActiveRuntimeKey(runtimeKey);
      setSelectedAgent(session.agent_id);
      setSelectedWorkspacePath(session.workspace);
      return runtimeKey;
    },
    [profileSelections, runtimeSnapshots, runtimeSpecs],
  );

  const removeRuntime = useCallback((runtimeKey: string) => {
    setRuntimeKeys((prev) => prev.filter((key) => key !== runtimeKey));
    setRuntimeSpecs((prev) => {
      const next = { ...prev };
      delete next[runtimeKey];
      return next;
    });
    setRuntimeSnapshots((prev) => {
      const next = { ...prev };
      delete next[runtimeKey];
      return next;
    });
    delete runtimeActionsRef.current[runtimeKey];
    delete syncedPromptDoneRef.current[runtimeKey];
    delete syncedActiveSessionRef.current[runtimeKey];
  }, []);

  useEffect(() => {
    onStatusChange?.(chatStatus);
  }, [chatStatus, onStatusChange]);

  useEffect(() => {
    if (!selectedAgent) return;
    setProfileSelections((prev) =>
      prev[selectedAgent] === undefined
        ? { ...prev, [selectedAgent]: DIRECT_PROFILE_ID }
        : prev,
    );
  }, [selectedAgent]);

  useEffect(() => {
    if (!selectedAgent || profiles.length === 0) return;
    const profileId = profileSelections[selectedAgent] ?? DIRECT_PROFILE_ID;
    if (profileId === DIRECT_PROFILE_ID) return;
    const profile = profiles.find((item) => item.id === profileId);
    if (profile && profileTargetsAgent(profile, selectedAgent)) return;
    setProfileSelections((prev) => ({ ...prev, [selectedAgent]: DIRECT_PROFILE_ID }));
  }, [profiles, profileSelections, selectedAgent]);

  useEffect(() => {
    if (!selectedAgent) return;
    writeStoredLaunchSelection({
      agentId: selectedAgent,
      profileId: profileSelections[selectedAgent] ?? DIRECT_PROFILE_ID,
    });
  }, [profileSelections, selectedAgent]);

  useEffect(() => {
    if (agents.length === 0) return;
    if (agents.some((agent) => agent.id === selectedAgent)) return;
    setSelectedAgent(agents[0]?.id ?? selectedAgent);
  }, [agents, selectedAgent]);

  useEffect(() => {
    let cancelled = false;
    setWorkspacesLoading(true);
    void getWorkspaces()
      .then(({ workspaces, default_workspace }) => {
        if (!cancelled) {
          setWorkspaces(workspaces);
          setDefaultWorkspacePath(default_workspace);
        }
      })
      .catch((error) => {
        if (!cancelled) {
          console.warn("[ChatView] failed to load workspaces:", error);
          setWorkspaces([]);
          setDefaultWorkspacePath(undefined);
        }
      })
      .finally(() => {
        if (!cancelled) setWorkspacesLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    setSelectedWorkspacePath((current) => {
      if (current && workspaces.some((workspace) => workspace.path === current)) {
        return current;
      }
      return workspaces[0]?.path;
    });
  }, [workspaces]);

  useEffect(() => {
    if (agents.length === 0) return;
    setSidebarAgentFilter((current) => {
      if (current === ALL_AGENTS_FILTER || agents.some((agent) => agent.id === current)) {
        return current;
      }
      return ALL_AGENTS_FILTER;
    });
  }, [agents]);

  useEffect(() => {
    let cancelled = false;
    void getProfiles()
      .then((items) => {
        if (!cancelled) setProfiles(items);
      })
      .catch((error) => {
        if (!cancelled) {
          console.warn("[ChatView] failed to load profiles:", error);
          setProfiles([]);
        }
      });

    return () => {
      cancelled = true;
    };
  }, []);

  const syncLaunchSessions = useCallback(
    async (options?: { force?: boolean; agentIds?: string[] }) => {
      const agentIds = agents.map((agent) => agent.id);
      const requestedAgentIds =
        options?.agentIds?.filter((agentId) => agentIds.includes(agentId)) ?? agentIds;
      if (agentIds.length === 0 || workspacesLoading || workspaces.length === 0) {
        syncedSessionScopeRef.current = undefined;
        syncedLaunchSessionAgentsRef.current = new Set();
        setSyncedLaunchSessionGroups([]);
        setSessionsLoading(false);
        return;
      }
      if (requestedAgentIds.length === 0) return;

      const scope = sessionSyncScope(agentIds, workspaces, webSettings.show_archived);
      const previousScope = syncedSessionScopeRef.current;
      if (!options?.force && previousScope === scope) return;
      syncedSessionScopeRef.current = scope;
      const canMergeCurrentGroups = previousScope === scope;
      if (!canMergeCurrentGroups) {
        syncedLaunchSessionAgentsRef.current = new Set();
      }

      const cachedGroups = canMergeCurrentGroups
        ? undefined
        : readCachedLaunchSessionGroups(scope, workspaces);
      if (cachedGroups) {
        syncedLaunchSessionAgentsRef.current = new Set(agentIds);
        setSyncedLaunchSessionGroups((currentGroups) =>
          mergeSessionGroupUpdates(
            canMergeCurrentGroups ? currentGroups : [],
            cachedGroups,
            workspaces,
            agentIds,
          ),
        );
      } else if (!canMergeCurrentGroups) {
        setSyncedLaunchSessionGroups([]);
      }

      const requestId = ++sessionSyncRequestIdRef.current;
      setSessionsLoading(true);
      try {
        const freshSessions = await getLaunchSessionsBatch(
          requestedAgentIds,
          webSettings.show_archived,
          workspaces.map((workspace) => workspace.path),
        );
        const sessionsByWorkspace = new Map<string, LaunchSessionInfo[]>();
        for (const session of freshSessions) {
          const sessions = sessionsByWorkspace.get(session.workspace) ?? [];
          sessions.push(session);
          sessionsByWorkspace.set(session.workspace, sessions);
        }
        const freshGroups = workspaces.map((workspace) => ({
          workspace,
          sessions: sessionsByWorkspace.get(workspace.path) ?? [],
        }));
        if (sessionSyncRequestIdRef.current !== requestId) return;
        setSyncedLaunchSessionGroups((currentGroups) => {
          const baseGroups =
            cachedGroups && !canMergeCurrentGroups
              ? mergeSessionGroupUpdates([], cachedGroups, workspaces, agentIds)
              : currentGroups;
          const mergedGroups = mergeSessionGroupUpdates(
            baseGroups,
            freshGroups,
            workspaces,
            requestedAgentIds,
          );
          syncedLaunchSessionAgentsRef.current = new Set([
            ...syncedLaunchSessionAgentsRef.current,
            ...requestedAgentIds,
          ]);
          if (agentIds.every((agentId) => syncedLaunchSessionAgentsRef.current.has(agentId))) {
            writeCachedLaunchSessionGroups(scope, mergedGroups);
          }
          return mergedGroups;
        });
      } catch (error) {
        console.warn("[ChatView] failed to sync launch sessions:", error);
      } finally {
        if (sessionSyncRequestIdRef.current === requestId) {
          setSessionsLoading(false);
        }
      }
    },
    [agents, webSettings.show_archived, workspaces, workspacesLoading],
  );

  useEffect(() => {
    void syncLaunchSessions();
  }, [syncLaunchSessions]);

  useEffect(() => {
    const agentIds = new Set<string>();
    for (const [runtimeKey, snapshot] of Object.entries(runtimeSnapshots)) {
      const lastPromptDoneAt = snapshot.lastPromptDoneAt;
      if (!lastPromptDoneAt) continue;
      if (syncedPromptDoneRef.current[runtimeKey] === lastPromptDoneAt) continue;
      syncedPromptDoneRef.current[runtimeKey] = lastPromptDoneAt;
      const agentId = runtimeSpecs[runtimeKey]?.agentId;
      if (agentId) agentIds.add(agentId);
    }
    if (agentIds.size === 0) return;
    void syncLaunchSessions({ force: true, agentIds: Array.from(agentIds) });
  }, [runtimeSnapshots, runtimeSpecs, syncLaunchSessions]);

  useEffect(() => {
    const agentIds = new Set<string>();
    for (const [runtimeKey, snapshot] of Object.entries(runtimeSnapshots)) {
      const spec = runtimeSpecs[runtimeKey];
      if (!spec) continue;
      const active = snapshot.streaming || Boolean(snapshot.resumeReplay);
      const sessionId = spec.launchSession?.session_id ?? snapshot.meta.sessionId;
      const workspace =
        spec.launchSession?.workspace ??
        spec.workspacePath ??
        snapshot.resumeReplay?.workspace;
      const activeKey =
        active && sessionId && workspace
          ? `${spec.agentId}\u0000${workspace}\u0000${sessionId}`
          : undefined;
      const previousActiveKey = syncedActiveSessionRef.current[runtimeKey];
      if (previousActiveKey === activeKey) continue;
      syncedActiveSessionRef.current[runtimeKey] = activeKey;
      if (activeKey || previousActiveKey) agentIds.add(spec.agentId);
    }
    if (agentIds.size === 0) return;
    void syncLaunchSessions({ force: true, agentIds: Array.from(agentIds) });
  }, [runtimeSnapshots, runtimeSpecs, syncLaunchSessions]);

  useEffect(() => {
    if (restoredActiveLaunchSessionRef.current) return;
    const stored = readStoredActiveLaunchSession();
    if (!stored) {
      restoredActiveLaunchSessionRef.current = true;
      return;
    }
    const session = storedActiveLaunchSessionToInfo(stored);
    restoredActiveLaunchSessionRef.current = true;
    setSessionSelections((prev) => ({
      ...prev,
      [session.agent_id]: { kind: "resume", sessionId: session.session_id },
    }));
    setSelectedLaunchSessions((prev) => ({
      ...prev,
      [session.agent_id]: session,
    }));
    storedActiveLaunchSessionKeyRef.current = chatSessionKey(session);
    activateRuntimeForSession(session);
  }, [activateRuntimeForSession]);

  useEffect(() => {
    const spec = runtimeSpecs[activeRuntimeKey];
    const snapshot = runtimeSnapshots[activeRuntimeKey];
    if (!spec || !snapshot) return;
    const sessionId = spec.launchSession?.session_id ?? snapshot.meta.sessionId;
    const workspace =
      spec.launchSession?.workspace ??
      spec.workspacePath ??
      snapshot.resumeReplay?.workspace ??
      selectedWorkspace?.path ??
      defaultWorkspacePath;
    if (!sessionId || !workspace) return;
    const key = `${spec.agentId}\u0000${workspace}\u0000${sessionId}`;
    if (storedActiveLaunchSessionKeyRef.current === key) return;
    writeStoredActiveLaunchSession(
      storedActiveLaunchSessionFromInfo({
        agent_id: spec.launchSession?.agent_id ?? spec.agentId,
        session_id: sessionId,
        workspace,
        title:
          spec.launchSession?.title ??
          spec.title ??
          snapshot.resumeReplay?.title ??
          t("Current session"),
        updated_at: Math.max(
          spec.lastPromptAt ?? 0,
          spec.launchSession?.updated_at ?? 0,
          snapshot.resumeReplay?.updatedAt ?? 0,
        ),
        short_id: spec.launchSession?.short_id ?? shortSessionId(sessionId),
        archived: spec.launchSession?.archived ?? false,
        active: true,
      }),
    );
    storedActiveLaunchSessionKeyRef.current = key;
  }, [
    activeRuntimeKey,
    defaultWorkspacePath,
    runtimeSnapshots,
    runtimeSpecs,
    selectedWorkspace?.path,
    t,
  ]);

  const handleLaunchChange = useCallback((agentId: string, profileId?: string) => {
    setSelectedAgent(agentId);
    setProfileSelections((prev) => {
      return { ...prev, [agentId]: profileId ?? DIRECT_PROFILE_ID };
    });
  }, []);

  const handleSidebarAgentFilterChange = useCallback((agentId: string) => {
    setSidebarAgentFilter(agentId);
    void syncLaunchSessions({
      force: true,
      agentIds: agentId === ALL_AGENTS_FILTER ? undefined : [agentId],
    });
  }, [syncLaunchSessions]);

  const handleSyncSessions = useCallback(() => {
    void syncLaunchSessions({ force: true });
  }, [syncLaunchSessions]);

  const handleCreateWorkspace = useCallback(async (name: string) => {
    setWorkspaceCreating(true);
    setWorkspaceCreateError(undefined);
    try {
      const response = await createWorkspace(name);
      setWorkspaces(response.workspaces);
      setDefaultWorkspacePath(response.default_workspace);
      setSelectedWorkspacePath(response.workspace.path);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setWorkspaceCreateError(message);
    } finally {
      setWorkspaceCreating(false);
    }
  }, []);

  const handleSessionChange = useCallback(
    (selection: ChatSessionSelection, session?: LaunchSessionInfo) => {
      const targetAgentId =
        session?.agent_id ??
        (sidebarAgentFilter === ALL_AGENTS_FILTER ? selectedAgent : sidebarAgentFilter);
      if (selection.kind === "new") {
        clearStoredActiveLaunchSession();
        storedActiveLaunchSessionKeyRef.current = undefined;
        setSessionSelections((prev) => ({ ...prev, [targetAgentId]: selection }));
        setSelectedLaunchSessions((prev) => {
          const next = { ...prev };
          delete next[targetAgentId];
          return next;
        });
        createDraftRuntime(targetAgentId, selectedWorkspace?.path);
        clearAttachments();
        return;
      }
      if (selection.kind !== "resume") return;

      const launchSession = session ?? launchSessions.find(
        (item) =>
          item.agent_id === targetAgentId && item.session_id === selection.sessionId,
      );
      if (!launchSession) return;

      setSessionSelections((prev) => ({ ...prev, [launchSession.agent_id]: selection }));
      setSelectedLaunchSessions((prev) => ({
        ...prev,
        [launchSession.agent_id]: launchSession,
      }));
      writeStoredActiveLaunchSession(storedActiveLaunchSessionFromInfo(launchSession));
      storedActiveLaunchSessionKeyRef.current = chatSessionKey(launchSession);
      clearAttachments();
      activateRuntimeForSession(launchSession);
    },
    [
      activateRuntimeForSession,
      clearAttachments,
      createDraftRuntime,
      launchSessions,
      selectedAgent,
      sidebarAgentFilter,
      selectedWorkspace?.path,
    ],
  );

  const handleMobileSessionChange = useCallback(
    (selection: ChatSessionSelection, session?: LaunchSessionInfo) => {
      handleSessionChange(selection, session);
      setMobileSessionSidebarOpen(false);
    },
    [handleSessionChange],
  );

  const handleSessionSidebarResizeStart = useCallback(
    (event: React.PointerEvent<HTMLButtonElement>) => {
      event.preventDefault();
      const startX = event.clientX;
      const startWidth = sessionSidebarWidth;
      let nextWidth = startWidth;
      const previousCursor = document.body.style.cursor;
      const previousUserSelect = document.body.style.userSelect;

      const handlePointerMove = (moveEvent: PointerEvent) => {
        nextWidth = clampSessionSidebarWidth(
          startWidth + moveEvent.clientX - startX,
        );
        setSessionSidebarWidth(nextWidth);
      };
      const handlePointerUp = () => {
        window.removeEventListener("pointermove", handlePointerMove);
        window.removeEventListener("pointerup", handlePointerUp);
        document.body.style.cursor = previousCursor;
        document.body.style.userSelect = previousUserSelect;
        writeStoredSessionSidebarWidth(nextWidth);
      };

      document.body.style.cursor = "col-resize";
      document.body.style.userSelect = "none";
      window.addEventListener("pointermove", handlePointerMove);
      window.addEventListener("pointerup", handlePointerUp, { once: true });
    },
    [sessionSidebarWidth],
  );

  const handleArchiveSession = useCallback(
    async (session: LaunchSessionInfo) => {
      setArchivingSessionId(session.session_id);
      try {
        const runtimeEntry = Object.entries(runtimeSpecs).find(([runtimeKey, spec]) => {
          const snapshot = runtimeSnapshots[runtimeKey];
          const sessionId = spec.launchSession?.session_id ?? snapshot?.meta.sessionId;
          const workspace =
            spec.launchSession?.workspace ?? spec.workspacePath ?? defaultWorkspacePath;
          return (
            spec.agentId === session.agent_id &&
            sessionId === session.session_id &&
            workspace === session.workspace
          );
        });
        if (runtimeEntry) {
          runtimeActionsRef.current[runtimeEntry[0]]?.stopStreaming();
        }
        await archiveLaunchSession(session.agent_id, session.session_id, session.workspace);
        void deleteCachedChatSession({
          agentId: session.agent_id,
          workspace: session.workspace,
          sessionId: session.session_id,
        }).catch((error) => {
          console.warn("[ChatView] failed to delete archived session cache:", error);
        });
        setSyncedLaunchSessionGroups((prev) => {
          const next = prev.map((group) => ({
            ...group,
            sessions: webSettings.show_archived
              ? group.sessions.map((item) =>
                  item.agent_id === session.agent_id &&
                  item.session_id === session.session_id
                    ? { ...item, archived: true }
                    : item,
                )
              : group.sessions.filter(
                  (item) =>
                    item.agent_id !== session.agent_id ||
                    item.session_id !== session.session_id,
                ),
          }));
          if (syncedSessionScopeRef.current) {
            writeCachedLaunchSessionGroups(syncedSessionScopeRef.current, next);
          }
          return next;
        });
        setSelectedLaunchSessions((prev) => {
          if (prev[session.agent_id]?.session_id !== session.session_id) return prev;
          const next = { ...prev };
          delete next[session.agent_id];
          return next;
        });
        setSessionSelections((prev) => {
          const current = prev[session.agent_id];
          if (current?.kind !== "resume" || current.sessionId !== session.session_id) {
            return prev;
          }
          return { ...prev, [session.agent_id]: { kind: "new" } };
        });
        if (
          selectedAgent === session.agent_id &&
          sessionSelection.kind === "resume" &&
          sessionSelection.sessionId === session.session_id
        ) {
          createDraftRuntime(session.agent_id, session.workspace);
        }
        if (runtimeEntry) {
          removeRuntime(runtimeEntry[0]);
        }
      } catch (error) {
        console.warn("[ChatView] failed to archive launch session:", error);
      } finally {
        setArchivingSessionId(undefined);
      }
    },
    [
      createDraftRuntime,
      defaultWorkspacePath,
      removeRuntime,
      runtimeSnapshots,
      runtimeSpecs,
      selectedAgent,
      sessionSelection,
      webSettings.show_archived,
    ],
  );

  const handleSubmit = useCallback(() => {
    const text = input.trim();
    if (!text && attachments.length === 0) return;
    if (attachmentsUploading) return;
    if (replayBlocksInput) return;
    if (!sendMessage) return;
    const messageWorkspacePath = selectedWorkspace?.path ?? defaultWorkspacePath;
    const sent = sendMessage({
      text,
      attachments,
      agentId: selectedAgent,
      profileId: selectedProfileId,
      workspacePath: messageWorkspacePath,
      sessionSelection,
      launchSession: selectedLaunchSession,
    });
    if (!sent) return;

    const promptSubmittedAt = currentUnixSeconds();
    setInput("");
    clearAttachments();
    setRuntimeSpecs((prev) => ({
      ...prev,
      [activeRuntimeKey]: {
        ...(prev[activeRuntimeKey] ?? { agentId: selectedAgent }),
        agentId: selectedAgent,
        profileId: selectedProfileId,
        workspacePath: messageWorkspacePath,
        launchSession: selectedLaunchSession,
        lastPromptAt: promptSubmittedAt,
        title:
          text ||
          attachments[0]?.name ||
          selectedLaunchSession?.title ||
          t("Current session"),
      },
    }));
    if (sessionSelection.kind === "new") {
      setSessionSelections((prev) => ({ ...prev, [selectedAgent]: { kind: "current" } }));
    }
  }, [
    activeRuntimeKey,
    attachments,
    attachmentsUploading,
    clearAttachments,
    input,
    replayBlocksInput,
    defaultWorkspacePath,
    selectedAgent,
    selectedLaunchSession,
    selectedProfileId,
    selectedWorkspace?.path,
    sendMessage,
    sessionSelection,
    t,
  ]);

  const handleSessionModeChange = useCallback(
    (value: string) => {
      if (!sessionMode) return;
      if (sessionMode.source === "config_option") {
        if (sessionMode.configId) {
          setSessionConfigOption?.(sessionMode.configId, value);
        }
        return;
      }
      setSessionMode?.(value);
    },
    [sessionMode, setSessionConfigOption, setSessionMode],
  );

  return (
    <div className="flex h-full overflow-hidden bg-background">
      {runtimeKeys.map((runtimeKey) => (
        <ChatRuntimeHost
          key={runtimeKey}
          runtimeKey={runtimeKey}
          initialResume={runtimeSpecs[runtimeKey]?.initialResume}
          onSnapshot={handleRuntimeSnapshot}
          onActions={handleRuntimeActions}
          onAgentSelected={handleSocketAgentSelected}
        />
      ))}
      {showSessionSidebar && (
        <div
          className="relative hidden h-full shrink-0 md:flex"
          style={{ width: sessionSidebarWidth }}
        >
          <ChatSessionSidebar
            workspaceGroups={displayLaunchSessionGroups}
            agents={agents}
            selectedAgentFilter={sidebarAgentFilter}
            activeAgentId={selectedAgent}
            className="flex w-full"
            style={{ width: "100%" }}
            sessionsLoading={sidebarSessionsLoading}
            loadingSessionId={resumeReplay?.sessionId}
            loadingSessionKeys={runtimeBusySessionKeys}
            archivingSessionId={archivingSessionId}
            sessionSelection={sessionSelection}
            onSyncSessions={handleSyncSessions}
            onAgentFilterChange={handleSidebarAgentFilterChange}
            onSessionChange={handleSessionChange}
            onArchiveSession={handleArchiveSession}
          />
          <button
            type="button"
            className="absolute inset-y-0 -right-1 z-10 w-2 cursor-col-resize touch-none rounded-sm bg-transparent transition-colors hover:bg-primary/25 focus-visible:bg-primary/25 focus-visible:outline-none"
            aria-label={t("Resize sessions")}
            title={t("Resize sessions")}
            onPointerDown={handleSessionSidebarResizeStart}
          />
        </div>
      )}
      {mobileSessionSidebarOpen && (
        <div className="fixed inset-0 z-40 md:hidden">
          <button
            type="button"
            className="absolute inset-0 bg-background/70 backdrop-blur-sm"
            aria-label={t("Close sessions")}
            onClick={() => setMobileSessionSidebarOpen(false)}
          />
          <div className="absolute inset-y-0 left-0 w-[min(18rem,86vw)] shadow-lg">
            <ChatSessionSidebar
              workspaceGroups={displayLaunchSessionGroups}
              agents={agents}
              selectedAgentFilter={sidebarAgentFilter}
              activeAgentId={selectedAgent}
              variant="mobile"
              sessionsLoading={sidebarSessionsLoading}
              loadingSessionId={resumeReplay?.sessionId}
              loadingSessionKeys={runtimeBusySessionKeys}
              archivingSessionId={archivingSessionId}
              sessionSelection={sessionSelection}
              onSyncSessions={handleSyncSessions}
              onAgentFilterChange={handleSidebarAgentFilterChange}
              onSessionChange={handleMobileSessionChange}
              onArchiveSession={handleArchiveSession}
            />
          </div>
        </div>
      )}

      <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
        <ChatHeader
          selectedAgent={selectedAgent}
          agentLabel={agentLabel}
          routeLabel={routeLabel}
          headerSessionLabel={headerSessionLabel}
          workspacePath={activeWorkspacePath}
          sessionId={meta.sessionId}
          chatStatus={chatStatus}
          statusLabel={statusLabel}
          connected={connected}
          streaming={streaming}
          replayLoading={replayLoading}
          showSessionSidebar={showSessionSidebar}
          onShowMobileSessions={() => setMobileSessionSidebarOpen(true)}
          onToggleSessionSidebar={() => setShowSessionSidebar((value) => !value)}
          onOpenAppSidebar={onOpenAppSidebar}
        />

        {showNewChatHome ? (
          <NewChatHome>
            <div className="space-y-4">
              <ChatInput
                value={input}
                onChange={setInput}
                onSubmit={handleSubmit}
                onStop={stopStreaming}
                attachments={attachments}
                attachmentsUploading={attachmentsUploading}
                attachmentsUploadingCount={attachmentsUploadingCount}
                attachmentError={attachmentError}
                onFilesSelected={handleFilesSelected}
                onRemoveAttachment={handleRemoveAttachment}
                disabled={!connected}
                submitDisabled={streaming || replayBlocksInput || attachmentsUploading}
                isStreaming={streaming}
                sendWithModifierEnter={webSettings.send_with_modifier_enter}
                sessionMode={sessionMode}
                onSessionModeChange={handleSessionModeChange}
                placeholder={
                  connected ? t("Ask {{agent}} anything…", { agent: agentLabel }) : t("Connecting…")
                }
                targetLabel={agentLabel}
                variant="hero"
              />
              <div className="grid gap-4 lg:grid-cols-2">
                <NewChatAgentPicker
                  agents={agents}
                  profiles={profiles}
                  selectedAgentId={selectedAgent}
                  selectedProfileId={selectedProfileId}
                  fallbackAgentLabel={agentLabel}
                  onLaunchChange={handleLaunchChange}
                  className="min-w-0"
                />
                <NewChatWorkspacePicker
                  workspaces={workspaces}
                  defaultWorkspacePath={defaultWorkspacePath}
                  selectedWorkspacePath={selectedWorkspace?.path}
                  loading={workspacesLoading}
                  creating={workspaceCreating}
                  createError={workspaceCreateError}
                  onWorkspaceChange={setSelectedWorkspacePath}
                  onCreateWorkspace={handleCreateWorkspace}
                  layout="panel"
                  className="min-w-0"
                />
              </div>
            </div>
          </NewChatHome>
        ) : (
          <>
            <ChatMessageList
              messages={messages}
              streaming={streaming}
              agentLabel={agentLabel}
              replayLoading={replayLoading}
              replayTitle={resumeReplay?.title}
              displaySettings={displaySettings}
              workspacePath={activeWorkspacePath}
            />

            <PendingPermissions
              permissions={pendingPermissions}
              onRespond={(requestId, optionId) =>
                sendPermissionResponse?.(requestId, optionId)
              }
              onCancel={(requestId) => cancelPermissionRequest?.(requestId)}
            />

            <ChatInput
              value={input}
              onChange={setInput}
              onSubmit={handleSubmit}
              onStop={stopStreaming}
              attachments={attachments}
              attachmentsUploading={attachmentsUploading}
              attachmentsUploadingCount={attachmentsUploadingCount}
              attachmentError={attachmentError}
              onFilesSelected={handleFilesSelected}
              onRemoveAttachment={handleRemoveAttachment}
              disabled={!connected || replayBlocksInput}
              submitDisabled={streaming || replayBlocksInput || attachmentsUploading}
              isStreaming={streaming}
              sendWithModifierEnter={webSettings.send_with_modifier_enter}
              sessionMode={sessionMode}
              onSessionModeChange={handleSessionModeChange}
              placeholder={
                connected ? t("Message {{agent}}…", { agent: agentLabel }) : t("Connecting…")
              }
              targetLabel={agentLabel}
            />
          </>
        )}
      </div>
      <SubagentPanel
        turns={multiAgentTurns}
        agents={subagents}
        messagesByAgent={activeRuntime.subagentMessages}
        displaySettings={displaySettings}
        open={subagentPanelOpen}
        selectedAgentId={selectedSubagentId}
        onOpenChange={setSubagentPanelOpen}
        onSelectedAgentChange={setSelectedSubagentId}
      />
    </div>
  );
}
