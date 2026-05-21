import { useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  FolderOpen,
  MessageCircle,
  Rocket,
  Terminal,
} from "lucide-react";
import { useI18n } from "@va/i18n";

import { TooltipProvider } from "@/components/ui/tooltip";
import { AgentRailButton, TooltipButton } from "./LaunchBuilderPrimitives";
import {
  ProfilePanel,
  SessionPanel,
  TerminalPanel,
  WorkspacePanel,
} from "./LaunchBuilderPanels";
import {
  AgentSummaryHeader,
  LaunchSummaryPill,
  ProfileInfoPanel,
  SelectorPopup,
  type SelectorPopupId,
} from "./LaunchSummary";
import {
  createProfile,
  deleteProfile,
  getLauncherPreferences,
  getProfile,
  launchDirect,
  launchDirectResume,
  launchProfile,
  launchProfileResume,
  listAgents,
  listLaunchSessions,
  listLauncherWorkspaces,
  listProfiles,
  removeLauncherWorkspace,
  reorderLauncherWorkspaces,
  reorderProfiles,
  setProfileConnection,
  setLauncherAgentProfile,
  setLauncherDefault,
  setLauncherSelectedAgent,
  setLauncherTerminal,
  setLauncherWorkspace,
  type AgentSummary,
  type LaunchSessionSummary,
  type LauncherPreferences,
  type WorkspaceOption,
} from "./api";
import { buildProfileCopyDraft } from "./profileClone";
import {
  agentLabel,
  agentProfileId,
  agentSupportsSessionResume,
  agentWorkspace,
  currentTerminal,
  currentWorkspace,
  isBridgeAgent,
  isSelectionLaunchable,
  isSortableWorkspace,
  mergeOrderedSubset,
  moveItemBefore,
  profileById,
  profileSupportsAgent,
  profileSummary,
  resolveSelectedSession,
  selectionUnavailableReason,
  type ProfileChoice,
  type SessionChoice,
} from "./launchModel";
import type {
  ConnectionAgentId,
  ProfileConnectionPreference,
  ProfileSummary,
} from "./types";

const AGENT_ORDER = [
  "codex",
  "claude",
  "pi",
  "gemini",
  "cursor",
  "kiro",
  "qwen-code",
  "opencode",
];
interface Props {
  profiles: ProfileSummary[];
  prefs: LauncherPreferences | null;
  onPrefsChange: (prefs: LauncherPreferences) => void;
  onProfilesChange: (profiles: ProfileSummary[]) => void;
  onNewProfile: () => void;
  onEditProfile: (profile: ProfileSummary) => void;
  onConnectionSettings: (
    profile: ProfileSummary,
    agentId: ConnectionAgentId,
  ) => void;
  onError: (message: string | null) => void;
  onToast: (message: string | null) => void;
}

export function AgentLaunchBuilder({
  profiles,
  prefs,
  onPrefsChange,
  onProfilesChange,
  onNewProfile,
  onEditProfile,
  onConnectionSettings,
  onError,
  onToast,
}: Props) {
  const { t } = useI18n();
  const [agents, setAgents] = useState<AgentSummary[]>([]);
  const [agentId, setAgentId] = useState<string>("");
  const [profileChoiceAgentId, setProfileChoiceAgentId] = useState<string>("");
  const [profileChoice, setProfileChoice] = useState<ProfileChoice>({
    kind: "direct",
  });
  const [openSelector, setOpenSelector] = useState<SelectorPopupId | null>(
    null,
  );
  const [workspaceOptions, setWorkspaceOptions] = useState<
    WorkspaceOption[] | null
  >(null);
  const [workspacesLoading, setWorkspacesLoading] = useState(false);
  const [sessions, setSessions] = useState<LaunchSessionSummary[]>([]);
  const [workspaceSessionCounts, setWorkspaceSessionCounts] = useState<
    Record<string, number>
  >({});
  const [sessionsLoading, setSessionsLoading] = useState(false);
  const [showArchivedSessions, setShowArchivedSessions] = useState(false);
  const [sessionChoice, setSessionChoice] = useState<SessionChoice>(null);
  const [busy, setBusy] = useState(false);

  const enabledAgents = useMemo(
    () => (prefs ? new Set(prefs.enabledAgents) : null),
    [prefs?.enabledAgents],
  );
  const viewPrefs = useMemo<LauncherPreferences | null>(() => {
    if (!prefs) return null;
    return {
      ...prefs,
      workspace: agentWorkspace(prefs, agentId),
      workspaceOptions: workspaceOptions ?? prefs.workspaceOptions,
    };
  }, [prefs, workspaceOptions, agentId]);

  useEffect(() => {
    void listAgents()
      .then((items) => {
        const rank = new Map(AGENT_ORDER.map((id, index) => [id, index]));
        const visible = enabledAgents
          ? items.filter((agent) => enabledAgents.has(agent.id))
          : items;
        const ordered = [...visible].sort(
          (a, b) => (rank.get(a.id) ?? 999) - (rank.get(b.id) ?? 999),
        );
        setAgents(ordered);
      })
      .catch((error) =>
        onError(error instanceof Error ? error.message : String(error)),
      );
  }, [enabledAgents, onError]);

  useEffect(() => {
    if (!prefs) return;
    if (agentId && agents.some((agent) => agent.id === agentId)) return;
    const preferredAgent = agents.some(
      (agent) => agent.id === prefs.selectedAgent,
    )
      ? prefs.selectedAgent
      : agents.some((agent) => agent.id === prefs.defaultAgent)
        ? prefs.defaultAgent
      : (agents[0]?.id ?? "");
    setAgentId(preferredAgent);
  }, [agentId, agents, prefs]);

  useEffect(() => {
    if (!prefs || !agentId) return;
    setProfileChoice((current) => {
      if (profileChoiceAgentId === agentId) {
        if (current.kind === "direct") return current;
        if (
          profileSupportsAgent(
            profileById(profiles, current.profileId),
            agentId,
            prefs,
          )
        ) {
          return current;
        }
      }

      const defaultProfileId = agentProfileId(prefs, agentId);
      if (
        defaultProfileId &&
        profileSupportsAgent(
          profileById(profiles, defaultProfileId),
          agentId,
          prefs,
        )
      ) {
        return { kind: "profile", profileId: defaultProfileId };
      }
      return { kind: "direct" };
    });
    setProfileChoiceAgentId(agentId);
    if (profileChoiceAgentId !== agentId) {
      setSessionChoice(null);
    }
  }, [agentId, prefs, profileChoiceAgentId, profiles]);

  useEffect(() => {
    if (!prefs || !agentId) {
      setWorkspaceOptions(null);
      setWorkspacesLoading(false);
      return;
    }

    let cancelled = false;
    setWorkspacesLoading(true);
    void listLauncherWorkspaces(agentId)
      .then((items) => {
        if (!cancelled) setWorkspaceOptions(items);
      })
      .catch((error) => {
        if (!cancelled) {
          onError(error instanceof Error ? error.message : String(error));
        }
      })
      .finally(() => {
        if (!cancelled) setWorkspacesLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [prefs, agentId, onError]);

  const currentAgentWorkspace = prefs ? agentWorkspace(prefs, agentId) : "";

  useEffect(() => {
    if (!agentId || !currentAgentWorkspace) {
      setSessions([]);
      setSessionsLoading(false);
      return;
    }
    if (!agentSupportsSessionResume(agentId)) {
      setSessions([]);
      setSessionChoice(null);
      setSessionsLoading(false);
      return;
    }
    let cancelled = false;
    setSessionsLoading(true);
    void listLaunchSessions(agentId, currentAgentWorkspace, showArchivedSessions)
      .then((items) => {
        if (cancelled) return;
        setSessions(items);
        setSessionChoice((current) =>
          current?.kind === "session" &&
          items.some((item) => item.sessionId === current.sessionId)
            ? current
            : null,
        );
      })
      .catch((error) => {
        if (!cancelled) {
          onError(error instanceof Error ? error.message : String(error));
        }
      })
      .finally(() => {
        if (!cancelled) setSessionsLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [agentId, currentAgentWorkspace, showArchivedSessions, onError]);

  useEffect(() => {
    if (!agentId || !agentSupportsSessionResume(agentId) || !viewPrefs) {
      setWorkspaceSessionCounts({});
      return;
    }
    let cancelled = false;
    const workspaces = viewPrefs.workspaceOptions;
    void Promise.all(
      workspaces.map((workspace) =>
        listLaunchSessions(agentId, workspace.path, false)
          .then((items) => [workspace.path, items.length] as const)
          .catch(() => [workspace.path, 0] as const),
      ),
    ).then((entries) => {
      if (!cancelled) {
        setWorkspaceSessionCounts(Object.fromEntries(entries));
      }
    });
    return () => {
      cancelled = true;
    };
  }, [agentId, viewPrefs?.workspaceOptions]);

  const selectedAgent = agents.find((agent) => agent.id === agentId);
  const selectedProfile =
    profileChoice.kind === "profile"
      ? profileById(profiles, profileChoice.profileId)
      : null;
  const profileOptions = profiles;
  const visibleSessions = useMemo(
    () =>
      showArchivedSessions
        ? sessions
        : sessions.filter((session) => !session.archived),
    [sessions, showArchivedSessions],
  );
  const selectedWorkspace = currentWorkspace(viewPrefs);
  const selectedTerminal = currentTerminal(viewPrefs);
  const selectedSession = resolveSelectedSession(
    sessionChoice,
    visibleSessions,
  );
  const selectionLaunchable = viewPrefs
    ? isSelectionLaunchable(profileChoice, selectedProfile, agentId, viewPrefs)
    : false;
  const selectionDisabledReason = viewPrefs
    ? selectionUnavailableReason(
        profileChoice,
        selectedProfile,
        agentId,
        viewPrefs,
        t,
      )
    : t("Loading…");
  const sessionResumeSupported = agentSupportsSessionResume(agentId);
  const sessionResumeUnsupportedReason = t(
    "{{agent}} does not support selecting a session to resume",
    { agent: agentLabel(agentId) },
  );
  const launchDisabledReason = busy
    ? t("Launch is already in progress")
    : selectionDisabledReason;

  async function refreshPrefs() {
    onPrefsChange(await getLauncherPreferences());
  }

  async function refreshProfiles() {
    onProfilesChange(await listProfiles());
  }

  async function chooseAgent(nextAgentId: string) {
    setAgentId(nextAgentId);
    setSessionChoice(null);
    onError(null);
    try {
      await setLauncherSelectedAgent(nextAgentId);
      await refreshPrefs();
    } catch (error) {
      onError(error instanceof Error ? error.message : String(error));
    }
  }

  async function chooseProfileChoice(choice: ProfileChoice) {
    setProfileChoice(choice);
    if (!agentId) return;
    onError(null);
    try {
      await setLauncherAgentProfile(
        agentId,
        choice.kind === "profile" ? choice.profileId : null,
      );
      await refreshPrefs();
    } catch (error) {
      onError(error instanceof Error ? error.message : String(error));
    }
  }

  async function chooseProfileApiType(profile: ProfileSummary, apiType: string) {
    if (!viewPrefs || !isBridgeAgent(agentId)) return;
    const current = viewPrefs.profileConnections[profile.id]?.[agentId] ?? {};
    onError(null);
    try {
      await setProfileConnection(profile.id, agentId, {
        ...current,
        selectedApiType: apiType,
      });
      await refreshPrefs();
    } catch (error) {
      onError(error instanceof Error ? error.message : String(error));
    }
  }

  async function chooseWorkspace(path: string) {
    if (!prefs || !agentId || path === agentWorkspace(prefs, agentId)) {
      return;
    }
    setBusy(true);
    onError(null);
    try {
      await setLauncherWorkspace(path, agentId);
      await refreshPrefs();
      setSessionChoice(null);
    } catch (error) {
      onError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  }

  async function chooseTerminal(terminalId: string) {
    if (!viewPrefs || terminalId === viewPrefs.terminal) return;
    setBusy(true);
    onError(null);
    try {
      await setLauncherTerminal(terminalId);
      await refreshPrefs();
    } catch (error) {
      onError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  }

  async function chooseFolder() {
    if (!agentId) return;
    setBusy(true);
    onError(null);
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: t("Choose Launch Workspace"),
      });
      const path = Array.isArray(selected) ? selected[0] : selected;
      if (!path) return;
      await setLauncherWorkspace(path, agentId);
      await refreshPrefs();
      setSessionChoice(null);
    } catch (error) {
      onError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  }

  async function makeDefault(choice: ProfileChoice) {
    setBusy(true);
    onError(null);
    try {
      await setLauncherDefault(
        agentId,
        choice.kind === "profile" ? choice.profileId : null,
      );
      await refreshPrefs();
      onToast(t("VibeAround default updated"));
    } catch (error) {
      onError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  }

  async function removeWorkspace(path: string, label: string) {
    if (!window.confirm(t('Delete workspace "{{label}}"?', { label }))) return;
    setBusy(true);
    onError(null);
    try {
      await removeLauncherWorkspace(path);
      await refreshPrefs();
      setSessionChoice(null);
      onToast(t("Workspace removed"));
    } catch (error) {
      onError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  }

  async function reorderWorkspace(fromPath: string, toPath: string) {
    if (!viewPrefs || fromPath === toPath) return;
    const reorderablePaths = viewPrefs.workspaceOptions
      .filter(isSortableWorkspace)
      .map((workspace) => workspace.path);
    const nextPaths = moveItemBefore(reorderablePaths, fromPath, toPath);
    if (nextPaths === reorderablePaths) return;
    setBusy(true);
    onError(null);
    try {
      await reorderLauncherWorkspaces(nextPaths);
      await refreshPrefs();
      onToast(t("Workspace order updated"));
    } catch (error) {
      onError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  }

  async function removeProfile(profile: ProfileSummary) {
    if (
      !window.confirm(
        t('Delete profile "{{label}}"?', { label: profile.label }),
      )
    )
      return;
    setBusy(true);
    onError(null);
    try {
      await deleteProfile(profile.id);
      await Promise.all([refreshProfiles(), refreshPrefs()]);
      onToast(t("Profile deleted"));
    } catch (error) {
      onError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  }

  async function copyProfile(profile: ProfileSummary) {
    if (!prefs) return;
    setBusy(true);
    onError(null);
    try {
      const fullProfile = await getProfile(profile.id);
      const copiedProfile = await createProfile(
        buildProfileCopyDraft(fullProfile, t("Copy")),
      );
      const sourceConnections = prefs.profileConnections[profile.id] ?? {};
      const connectionCopies = Object.entries(sourceConnections)
        .filter((entry): entry is [ConnectionAgentId, ProfileConnectionPreference] =>
          Boolean(entry[1]),
        )
        .map(([connectionAgentId, preference]) =>
          setProfileConnection(copiedProfile.id, connectionAgentId, preference),
        );
      await Promise.all(connectionCopies);
      await Promise.all([refreshProfiles(), refreshPrefs()]);
      onToast(t("Profile copied"));
    } catch (error) {
      onError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  }

  async function reorderProfile(fromId: string, toId: string) {
    if (fromId === toId) return;
    const visibleIds = profileOptions.map((profile) => profile.id);
    const movedVisibleIds = moveItemBefore(visibleIds, fromId, toId);
    if (movedVisibleIds === visibleIds) return;
    const nextIds = mergeOrderedSubset(
      profiles.map((profile) => profile.id),
      new Set(visibleIds),
      movedVisibleIds,
    );
    setBusy(true);
    onError(null);
    try {
      await reorderProfiles(nextIds);
      await refreshProfiles();
      onToast(t("Profile order updated"));
    } catch (error) {
      onError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  }

  async function launchSelected() {
    if (!agentId) return;
    setBusy(true);
    onError(null);
    try {
      if (selectedSession) {
        if (profileChoice.kind === "profile") {
          await launchProfileResume(
            profileChoice.profileId,
            agentId,
            selectedSession.sessionId,
          );
        } else {
          await launchDirectResume(agentId, selectedSession.sessionId);
        }
        onToast(t("Resume launch opened"));
      } else {
        if (profileChoice.kind === "profile") {
          await launchProfile(profileChoice.profileId, agentId);
        } else {
          await launchDirect(agentId);
        }
        onToast(t("Quick launch opened"));
      }
    } catch (error) {
      onError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  }

  if (!viewPrefs || agents.length === 0 || !selectedAgent) {
    if (prefs?.enabledAgents.length === 0) {
      return (
        <p className="p-3 text-xs text-muted-foreground">
          {t("No launch agents enabled")}
        </p>
      );
    }
    return <p className="p-3 text-xs text-muted-foreground">{t("Loading…")}</p>;
  }

  const selectedProfileSummary = selectedProfile
    ? profileSummary(selectedProfile, agentId, viewPrefs, t)
    : {
        title: t("Direct"),
        detail: t("Use existing CLI login"),
        bridge: false,
        route: t("Native CLI login"),
      };
  const sessionTitle = selectedSession?.title ?? t("New session");

  return (
    <TooltipProvider>
      <div className="flex min-h-0 flex-1">
        <aside className="w-[74px] shrink-0 border-r border-border bg-card/50 px-2 py-3">
          <div className="flex flex-col gap-2">
            {agents.map((agent) => (
              <AgentRailButton
                key={agent.id}
                agent={agent}
                active={agent.id === agentId}
                isDefault={viewPrefs.defaultAgent === agent.id}
                onClick={() => void chooseAgent(agent.id)}
              />
            ))}
          </div>
        </aside>

        <main className="flex min-w-0 flex-1 flex-col">
          <div className="flex min-h-0 flex-1 flex-col">
            <header className="bg-card/20 p-3">
              <div className="grid grid-cols-4 items-stretch gap-2">
                <div className="col-span-3 overflow-visible rounded-md border border-border bg-card p-3 shadow-sm">
                  <AgentSummaryHeader
                    agentId={agentId}
                    agentLabelText={selectedAgent.display_name}
                  >
                    <SelectorPopup
                      id="profile"
                      openSelector={openSelector}
                      onOpenChange={setOpenSelector}
                      widthClassName="w-max min-w-[340px] max-w-[min(680px,calc(100vw-1rem))]"
                      widthPx={680}
                      trigger={
                        <button
                          type="button"
                          className={`mt-0.5 flex max-w-[520px] min-w-0 cursor-pointer items-center gap-1 rounded-sm text-left text-[12px] leading-5 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring ${
                            openSelector === "profile"
                              ? "text-primary"
                              : "text-muted-foreground hover:text-foreground"
                          }`}
                          onClick={() =>
                            setOpenSelector(
                              openSelector === "profile" ? null : "profile",
                            )
                          }
                        >
                          <span className="min-w-0 truncate font-semibold text-foreground">
                            {selectedProfileSummary.title}
                          </span>
                          <span className="min-w-0 truncate text-muted-foreground">
                            <span className="px-0.5">·</span>
                            {selectedProfileSummary.route}
                          </span>
                        </button>
                      }
                    >
                      <ProfileInfoPanel
                        agentId={agentId}
                        prefs={viewPrefs}
                        profile={selectedProfile}
                        summary={selectedProfileSummary}
                      />
                    </SelectorPopup>
                  </AgentSummaryHeader>
                  <div className="mt-3 flex min-w-0 flex-wrap items-center gap-2">
                    <SelectorPopup
                      id="terminal"
                      openSelector={openSelector}
                      onOpenChange={setOpenSelector}
                      widthClassName="w-[min(300px,calc(100vw-1rem))]"
                      widthPx={300}
                      trigger={
                        <LaunchSummaryPill
                          active={openSelector === "terminal"}
                          chevron
                          disabled={busy}
                          className="w-[160px]"
                          icon={<Terminal className="h-4 w-4" />}
                          onClick={() =>
                            setOpenSelector(
                              openSelector === "terminal" ? null : "terminal",
                            )
                          }
                          label={t("Terminal")}
                          title={selectedTerminal?.label ?? t("Terminal")}
                        />
                      }
                    >
                      <TerminalPanel
                        options={viewPrefs.options}
                        selected={viewPrefs.terminal}
                        busy={busy}
                        onSelect={(terminalId) => {
                          setOpenSelector(null);
                          void chooseTerminal(terminalId);
                        }}
                      />
                    </SelectorPopup>
                    <SelectorPopup
                      id="workspace"
                      openSelector={openSelector}
                      onOpenChange={setOpenSelector}
                      widthClassName="w-[min(400px,calc(100vw-1rem))]"
                      widthPx={400}
                      trigger={
                        <LaunchSummaryPill
                          active={openSelector === "workspace"}
                          chevron
                          className="w-[250px]"
                          icon={<FolderOpen className="h-4 w-4" />}
                          onClick={() =>
                            setOpenSelector(
                              openSelector === "workspace" ? null : "workspace",
                            )
                          }
                          label={t("Workspace")}
                          title={selectedWorkspace.label}
                          detail={workspacesLoading ? t("Loading…") : undefined}
                        />
                      }
                    >
                      <WorkspacePanel
                        prefs={viewPrefs}
                        loading={workspacesLoading}
                        onSelect={(path) => {
                          setOpenSelector(null);
                          void chooseWorkspace(path);
                        }}
                        onDelete={(path, label) =>
                          void removeWorkspace(path, label)
                        }
                        onReorder={(fromPath, toPath) =>
                          void reorderWorkspace(fromPath, toPath)
                        }
                        onCreate={() => {
                          setOpenSelector(null);
                          void chooseFolder();
                        }}
                        sessionCounts={workspaceSessionCounts}
                        busy={busy}
                      />
                    </SelectorPopup>
                    <SelectorPopup
                      id="session"
                      openSelector={openSelector}
                      onOpenChange={setOpenSelector}
                      widthClassName="w-[min(420px,calc(100vw-1rem))]"
                      widthPx={420}
                      trigger={
                        <LaunchSummaryPill
                          active={openSelector === "session"}
                          chevron
                          disabled={!sessionResumeSupported}
                          className="w-[210px]"
                          icon={<MessageCircle className="h-4 w-4" />}
                          onClick={() =>
                            setOpenSelector(
                              openSelector === "session" ? null : "session",
                            )
                          }
                          label={t("Session")}
                          title={
                            !sessionResumeSupported
                              ? t("Session resume unavailable")
                              : sessionsLoading
                                ? t("Loading…")
                                : sessionTitle
                          }
                          detail={selectedSession ? t("resume") : undefined}
                        />
                      }
                    >
                      <SessionPanel
                        sessions={visibleSessions}
                        selected={sessionChoice}
                        archiveFilterAvailable={agentId === "codex"}
                        resumeSupported={sessionResumeSupported}
                        unsupportedReason={sessionResumeUnsupportedReason}
                        showArchived={showArchivedSessions}
                        onShowArchivedChange={setShowArchivedSessions}
                        onSelect={(choice) => {
                          setOpenSelector(null);
                          setSessionChoice(choice);
                        }}
                      />
                    </SelectorPopup>
                  </div>
                </div>
                <div className="col-span-1 flex">
                  <TooltipButton
                    type="button"
                    disabled={busy || !selectionLaunchable}
                    disabledReason={launchDisabledReason}
                    onClick={() => void launchSelected()}
                    size="lg"
                    className="h-full min-h-[115px] w-full justify-center gap-4 rounded-md text-[28px] font-semibold tracking-[0.12em] shadow-md shadow-primary/15 transition-none"
                  >
                    <Rocket className="size-8" />
                    {t("LAUNCH")}
                  </TooltipButton>
                </div>
              </div>
            </header>

            <section className="min-h-0 flex-1 overflow-y-auto px-3 pb-3">
              <ProfilePanel
                agentId={agentId}
                prefs={viewPrefs}
                selected={profileChoice}
                profiles={profileOptions}
                onSelect={(choice) => void chooseProfileChoice(choice)}
                onSelectApiType={(profile, apiType) =>
                  void chooseProfileApiType(profile, apiType)
                }
                onMakeDefault={makeDefault}
                onEditProfile={onEditProfile}
                onCopyProfile={(profile) => void copyProfile(profile)}
                onConnectionSettings={onConnectionSettings}
                onDeleteProfile={(profile) => void removeProfile(profile)}
                onReorderProfile={(fromId, toId) =>
                  void reorderProfile(fromId, toId)
                }
                onNewProfile={onNewProfile}
                busy={busy}
              />

            </section>
          </div>
        </main>
      </div>
    </TooltipProvider>
  );
}
