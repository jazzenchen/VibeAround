"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  archiveLaunchSession,
  createWorkspace,
  getLaunchSessionsBatch,
  getProfiles,
  getWorkspaces,
  initWorkspaceThread,
} from "@/api/sessions";
import { getAgentDisplayName } from "@/lib/agents";
import type {
  LaunchSessionInfo,
  ProfileLaunchOption,
  WebVerboseSettings,
  WorkspaceItem,
} from "@va/client";
import { useI18n } from "@va/i18n";
import { ChatHeader } from "./ChatHeader";
import { ChatRuntimeHost } from "./ChatRuntimeHost";
import {
  chatRuntimeKeyForSession,
  chatIdForThread,
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
import { NewChatAgentPicker } from "./NewChatAgentPicker";
import { NewChatHome } from "./NewChatHome";
import { NewChatWorkspacePicker } from "./NewChatWorkspacePicker";
import { ChatInput, ChatMessageList, PendingPermissions } from "./chatUi";
import { SubagentPanel } from "./SubagentPanel";
import { currentUnixSeconds } from "./chatTime";
import type { ChatSessionSelection } from "./chatTypes";
import { useChatAttachments } from "./useChatAttachments";

interface ChatViewProps {
  webSettings: WebVerboseSettings;
  onOpenAppSidebar?: () => void;
}

const DIRECT_PROFILE_ID = "direct";

export function ChatView({
  webSettings,
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
  const syncedTurnCompletionRef = useRef<Record<string, number>>({});
  const syncedActiveSessionRef = useRef<Record<string, string | undefined>>({});
  const runtimeThreadInitRef = useRef<Record<string, string>>({});

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
  const selectedAgentLabel = selectedAgentInfo?.name ?? getAgentDisplayName(selectedAgent);
  const selectedProfileId = profileSelections[selectedAgent] ?? DIRECT_PROFILE_ID;
  const profilesById = useMemo(
    () => new Map(profiles.map((profile) => [profile.id, profile])),
    [profiles],
  );
  const profileLabelForId = useCallback(
    (profileId?: string | null, fallback?: string | null) => {
      if (fallback) return fallback;
      if (!profileId) return undefined;
      if (profileId === DIRECT_PROFILE_ID) return t("Native");
      return profilesById.get(profileId)?.label ?? profileId;
    },
    [profilesById, t],
  );
  const activeSpec = runtimeSpecs[activeRuntimeKey];
  const activeLaunchSession = activeSpec?.launchSession;
  const activeAgentId =
    activeLaunchSession?.host_agent_id ??
    activeLaunchSession?.agent_id ??
    activeSpec?.agentId ??
    selectedAgent;
  const activeAgentInfo = agents.find((agent) => agent.id === activeAgentId);
  const agentLabel = activeAgentInfo?.name ?? getAgentDisplayName(activeAgentId);
  const activeProfileId = activeLaunchSession
    ? activeLaunchSession.host_profile_id
    : activeSpec?.profileId ?? selectedProfileId;
  const activeProfile =
    activeProfileId && activeProfileId !== DIRECT_PROFILE_ID
      ? profilesById.get(activeProfileId)
      : undefined;
  const activeProviderId = activeLaunchSession?.host_provider ?? activeProfile?.provider;
  const activeProfileLabel = profileLabelForId(
    activeProfileId,
    activeLaunchSession?.host_profile_label,
  );
  const activeProviderLabel =
    activeLaunchSession?.host_provider_label ?? activeProfileLabel;
  const selectedWorkspace = workspaces.find(
    (workspace) => workspace.path === selectedWorkspacePath,
  );
  const activeWorkspacePath =
    activeSpec?.launchSession?.workspace ??
    activeSpec?.workspacePath ??
    resumeReplay?.workspace ??
    selectedWorkspace?.path ??
    defaultWorkspacePath;
  const sessionSelection = sessionSelections[selectedAgent] ?? { kind: "new" };
  const activeSessionSelection =
    sessionSelections[activeAgentId] ?? sessionSelection;
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
  const headerSessionLabel =
    activeLaunchSession
      ? activeLaunchSession.title
      : activeSessionSelection.kind === "new"
        ? null
        : meta.sessionId
          ? t("Current session")
          : null;
  const routeLabel =
    activeProfileId
      ? t("{{agent}} / {{profile}}", {
          agent: agentLabel,
          profile: activeProfileLabel ?? activeProfileId,
        })
      : agentLabel;
  const showNewChatHome =
    messages.length === 0 &&
    !activeLaunchSession &&
    activeSessionSelection.kind !== "resume";
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
      const profileId = spec.launchSession?.host_profile_id ?? spec.profileId;
      const profile =
        profileId && profileId !== DIRECT_PROFILE_ID ? profilesById.get(profileId) : undefined;
      const profileLabel = profileLabelForId(
        profileId,
        spec.launchSession?.host_profile_label,
      );
      return [
        {
          agent_id: spec.launchSession?.agent_id ?? spec.agentId,
          host_agent_id: spec.launchSession?.host_agent_id ?? spec.agentId,
          host_profile_id: profileId,
          host_profile_label: profileLabel,
          host_provider: spec.launchSession?.host_provider ?? profile?.provider,
          host_provider_label: spec.launchSession?.host_provider_label ?? profileLabel,
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
          thread_id: spec.threadId ?? spec.launchSession?.thread_id,
        } satisfies LaunchSessionInfo,
      ];
    });
  }, [
    defaultWorkspacePath,
    profileLabelForId,
    runtimeSnapshots,
    runtimeSpecs,
    profilesById,
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

  const updateActiveDraftRuntime = useCallback(
    (agentId: string, profileId: string | undefined, workspacePath?: string) => {
      if (messages.length > 0 || sessionSelection.kind !== "new") return;
      delete runtimeThreadInitRef.current[activeRuntimeKey];
      setRuntimeSpecs((prev) => {
        const current = prev[activeRuntimeKey];
        if (!current || current.launchSession) return prev;
        return {
          ...prev,
          [activeRuntimeKey]: {
            ...current,
            agentId,
            profileId,
            workspacePath,
            threadId: undefined,
            chatId: undefined,
            launchSession: undefined,
            initialResume: undefined,
          },
        };
      });
    },
    [activeRuntimeKey, messages.length, sessionSelection.kind],
  );

  const activateRuntimeForSession = useCallback(
    (session: LaunchSessionInfo) => {
      const sessionProfileId = session.host_profile_id ?? undefined;
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
              profileId: sessionProfileId,
            }),
            agentId: session.agent_id,
            profileId: sessionProfileId,
            workspacePath: session.workspace,
            threadId: session.thread_id ?? prev[runtimeKey]?.threadId,
            chatId:
              session.thread_id !== undefined && session.thread_id !== null
                ? chatIdForThread(session.thread_id)
                : prev[runtimeKey]?.chatId,
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
                profileId: sessionProfileId,
                workspacePath: session.workspace,
                threadId: session.thread_id ?? undefined,
                chatId: session.thread_id ? chatIdForThread(session.thread_id) : undefined,
                launchSession: session,
                title: session.title,
                initialResume: {
                  agentId: session.agent_id,
                  profileId: sessionProfileId,
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

  useEffect(() => {
    const knownThreadUpdates = Object.entries(runtimeSpecs).filter(([, spec]) => {
      const threadId = spec.threadId ?? spec.launchSession?.thread_id ?? undefined;
      return Boolean(threadId) && (spec.threadId !== threadId || !spec.chatId);
    });
    if (knownThreadUpdates.length > 0) {
      setRuntimeSpecs((prev) => {
        let changed = false;
        const next = { ...prev };
        for (const [runtimeKey] of knownThreadUpdates) {
          const current = next[runtimeKey];
          if (!current) continue;
          const threadId = current.threadId ?? current.launchSession?.thread_id ?? undefined;
          if (!threadId) continue;
          const chatId = chatIdForThread(threadId);
          if (current.threadId === threadId && current.chatId === chatId) continue;
          next[runtimeKey] = { ...current, threadId, chatId };
          changed = true;
        }
        return changed ? next : prev;
      });
    }

    for (const [runtimeKey, spec] of Object.entries(runtimeSpecs)) {
      if (spec.threadId || spec.launchSession?.thread_id) continue;
      const workspacePath =
        spec.launchSession?.workspace ??
        spec.workspacePath ??
        selectedWorkspace?.path ??
        defaultWorkspacePath;
      if (!workspacePath) continue;
      const sessionId = spec.launchSession?.session_id;
      const profileId = spec.launchSession?.host_profile_id ?? spec.profileId;
      const signature = [
        spec.agentId,
        profileId ?? "",
        workspacePath,
        sessionId ?? "",
      ].join("\u0000");
      if (runtimeThreadInitRef.current[runtimeKey] === signature) continue;
      runtimeThreadInitRef.current[runtimeKey] = signature;

      void initWorkspaceThread({
        agent_id: spec.agentId,
        profile_id: profileId,
        session_id: sessionId,
        workspace_path: workspacePath,
      })
        .then((response) => {
          const threadId = response.thread_id;
          const chatId = response.chat_id || chatIdForThread(threadId);
          setRuntimeSpecs((prev) => {
            const current = prev[runtimeKey];
            if (!current) return prev;
            const currentWorkspace =
              current.launchSession?.workspace ??
              current.workspacePath ??
              selectedWorkspace?.path ??
              defaultWorkspacePath;
            const currentSignature = [
              current.agentId,
              current.launchSession?.host_profile_id ?? current.profileId ?? "",
              currentWorkspace ?? "",
              current.launchSession?.session_id ?? "",
            ].join("\u0000");
            if (currentSignature !== signature) return prev;
            const responseProfileId = response.profile_id ?? profileId;
            const responseProfile =
              responseProfileId && responseProfileId !== DIRECT_PROFILE_ID
                ? profilesById.get(responseProfileId)
                : undefined;
            const responseProfileLabel = profileLabelForId(responseProfileId);
            const launchSession = current.launchSession
              ? {
                  ...current.launchSession,
                  session_id: response.session_id ?? current.launchSession.session_id,
                  workspace: response.workspace || current.launchSession.workspace,
                  thread_id: threadId,
                  host_agent_id: response.agent_id || current.launchSession.host_agent_id,
                  host_profile_id:
                    responseProfileId ?? current.launchSession.host_profile_id,
                  host_profile_label:
                    responseProfileLabel ?? current.launchSession.host_profile_label,
                  host_provider:
                    responseProfile?.provider ?? current.launchSession.host_provider,
                  host_provider_label:
                    responseProfileLabel ?? current.launchSession.host_provider_label,
                }
              : undefined;
            const initialResume = current.initialResume
              ? {
                  ...current.initialResume,
                  launchSession: launchSession ?? current.initialResume.launchSession,
                }
              : undefined;
            return {
              ...prev,
              [runtimeKey]: {
                ...current,
                agentId: response.agent_id || current.agentId,
                profileId: responseProfileId ?? current.profileId,
                workspacePath: response.workspace || current.workspacePath,
                threadId,
                chatId,
                launchSession,
                initialResume,
              },
            };
          });

          const launchSession = spec.launchSession;
          if (launchSession) {
            const responseProfileId = response.profile_id ?? profileId;
            const responseProfile =
              responseProfileId && responseProfileId !== DIRECT_PROFILE_ID
                ? profilesById.get(responseProfileId)
                : undefined;
            const responseProfileLabel = profileLabelForId(responseProfileId);
            const launchSessionUpdate = {
              thread_id: threadId,
              host_agent_id: response.agent_id || launchSession.host_agent_id,
              host_profile_id: responseProfileId ?? launchSession.host_profile_id,
              host_profile_label:
                responseProfileLabel ?? launchSession.host_profile_label,
              host_provider: responseProfile?.provider ?? launchSession.host_provider,
              host_provider_label:
                responseProfileLabel ?? launchSession.host_provider_label,
            };
            setSelectedLaunchSessions((prev) => {
              const current = prev[launchSession.agent_id];
              if (
                !current ||
                current.session_id !== launchSession.session_id ||
                current.workspace !== launchSession.workspace
              ) {
                return prev;
              }
              return {
                ...prev,
                [launchSession.agent_id]: { ...current, ...launchSessionUpdate },
              };
            });
            setSyncedLaunchSessionGroups((prev) =>
              prev.map((group) => ({
                ...group,
                sessions: group.sessions.map((session) =>
                  session.agent_id === launchSession.agent_id &&
                  session.session_id === launchSession.session_id &&
                  session.workspace === launchSession.workspace
                    ? { ...session, ...launchSessionUpdate }
                    : session,
                ),
              })),
            );
          }
        })
        .catch((error) => {
          delete runtimeThreadInitRef.current[runtimeKey];
          console.warn("[ChatView] failed to initialize workspace thread:", error);
        });
    }
  }, [
    defaultWorkspacePath,
    profileLabelForId,
    profilesById,
    runtimeSpecs,
    selectedWorkspace?.path,
  ]);

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
    delete syncedTurnCompletionRef.current[runtimeKey];
    delete syncedActiveSessionRef.current[runtimeKey];
  }, []);

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
      const lastTurnCompletedAt = snapshot.lastTurnCompletedAt;
      if (!lastTurnCompletedAt) continue;
      if (syncedTurnCompletionRef.current[runtimeKey] === lastTurnCompletedAt) continue;
      syncedTurnCompletionRef.current[runtimeKey] = lastTurnCompletedAt;
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
          ? chatSessionKey({
              agent_id: spec.agentId,
              workspace,
              session_id: sessionId,
              thread_id: spec.threadId ?? spec.launchSession?.thread_id,
            })
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
    const storedProfileId = spec.launchSession?.host_profile_id ?? spec.profileId;
    const storedProfileLabel = profileLabelForId(
      storedProfileId,
      spec.launchSession?.host_profile_label,
    );
    const activeSessionInfo = {
      agent_id: spec.launchSession?.agent_id ?? spec.agentId,
      host_agent_id: spec.launchSession?.host_agent_id ?? spec.agentId,
      host_profile_id: storedProfileId,
      host_profile_label: storedProfileLabel,
      host_provider:
        spec.launchSession?.host_provider ??
        (storedProfileId && storedProfileId !== DIRECT_PROFILE_ID
          ? profilesById.get(storedProfileId)?.provider
          : undefined),
      host_provider_label:
        spec.launchSession?.host_provider_label ?? storedProfileLabel,
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
      thread_id: spec.threadId ?? spec.launchSession?.thread_id,
    } satisfies LaunchSessionInfo;
    const key = chatSessionKey(activeSessionInfo);
    if (storedActiveLaunchSessionKeyRef.current === key) return;
    writeStoredActiveLaunchSession(storedActiveLaunchSessionFromInfo(activeSessionInfo));
    storedActiveLaunchSessionKeyRef.current = key;
  }, [
    activeRuntimeKey,
    defaultWorkspacePath,
    profileLabelForId,
    profilesById,
    runtimeSnapshots,
    runtimeSpecs,
    selectedWorkspace?.path,
    t,
  ]);

  const handleLaunchChange = useCallback((agentId: string, profileId?: string) => {
    const nextProfileId = profileId ?? DIRECT_PROFILE_ID;
    setSelectedAgent(agentId);
    setProfileSelections((prev) => {
      return { ...prev, [agentId]: nextProfileId };
    });
    updateActiveDraftRuntime(
      agentId,
      nextProfileId,
      selectedWorkspace?.path ?? defaultWorkspacePath,
    );
  }, [defaultWorkspacePath, selectedWorkspace?.path, updateActiveDraftRuntime]);

  const handleWorkspaceSelectionChange = useCallback(
    (workspacePath: string) => {
      setSelectedWorkspacePath(workspacePath);
      updateActiveDraftRuntime(selectedAgent, selectedProfileId, workspacePath);
    },
    [selectedAgent, selectedProfileId, updateActiveDraftRuntime],
  );

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
      updateActiveDraftRuntime(selectedAgent, selectedProfileId, response.workspace.path);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setWorkspaceCreateError(message);
    } finally {
      setWorkspaceCreating(false);
    }
  }, [selectedAgent, selectedProfileId, updateActiveDraftRuntime]);

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
    const messageAgentId = activeSpec?.agentId ?? selectedAgent;
    const messageProfileId = activeSpec?.profileId ?? selectedProfileId;
    const messageLaunchSession = activeLaunchSession ?? selectedLaunchSession;
    const messageSessionSelection =
      activeSessionSelection.kind === "new" && activeLaunchSession
        ? { kind: "resume" as const, sessionId: activeLaunchSession.session_id }
        : activeSessionSelection;
    const sent = sendMessage({
      text,
      attachments,
      agentId: messageAgentId,
      profileId: messageProfileId,
      workspacePath: messageWorkspacePath,
      threadId: activeSpec?.threadId,
      sessionSelection: messageSessionSelection,
      launchSession: messageLaunchSession,
    });
    if (!sent) return;

    const promptSubmittedAt = currentUnixSeconds();
    setInput("");
    clearAttachments();
    setRuntimeSpecs((prev) => ({
      ...prev,
      [activeRuntimeKey]: {
        ...(prev[activeRuntimeKey] ?? { agentId: messageAgentId }),
        agentId: messageAgentId,
        profileId: messageProfileId,
        workspacePath: messageWorkspacePath,
        launchSession: messageLaunchSession,
        lastPromptAt: promptSubmittedAt,
        title:
          text ||
          attachments[0]?.name ||
          messageLaunchSession?.title ||
          t("Current session"),
      },
    }));
    if (messageSessionSelection.kind === "new") {
      setSessionSelections((prev) => ({ ...prev, [messageAgentId]: { kind: "current" } }));
    }
  }, [
    activeRuntimeKey,
    activeLaunchSession,
    activeSessionSelection,
    activeSpec,
    attachments,
    attachmentsUploading,
    clearAttachments,
    input,
    replayBlocksInput,
    defaultWorkspacePath,
    runtimeSpecs,
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
          chatId={runtimeSpecs[runtimeKey]?.chatId}
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
            activeAgentId={activeAgentId}
            className="flex w-full"
            style={{ width: "100%" }}
            sessionsLoading={sidebarSessionsLoading}
            loadingSessionId={resumeReplay?.sessionId}
            loadingSessionKeys={runtimeBusySessionKeys}
            archivingSessionId={archivingSessionId}
            sessionSelection={activeSessionSelection}
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
              activeAgentId={activeAgentId}
              variant="mobile"
              sessionsLoading={sidebarSessionsLoading}
              loadingSessionId={resumeReplay?.sessionId}
              loadingSessionKeys={runtimeBusySessionKeys}
              archivingSessionId={archivingSessionId}
              sessionSelection={activeSessionSelection}
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
          selectedAgent={activeAgentId}
          agentLabel={agentLabel}
          profileId={activeProfileId}
          profileLabel={activeProfileLabel}
          providerId={activeProviderId}
          providerLabel={activeProviderLabel}
          routeLabel={routeLabel}
          headerSessionLabel={headerSessionLabel}
          workspacePath={activeWorkspacePath}
          sessionId={meta.sessionId}
          connected={connected}
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
                targetLabel={routeLabel}
                variant="hero"
                className="pb-1"
              />
              <div className="space-y-4">
                <NewChatAgentPicker
                  agents={agents}
                  profiles={profiles}
                  selectedAgentId={selectedAgent}
                  selectedProfileId={selectedProfileId}
                  fallbackAgentLabel={selectedAgentLabel}
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
                  onWorkspaceChange={handleWorkspaceSelectionChange}
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
              targetLabel={routeLabel}
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
