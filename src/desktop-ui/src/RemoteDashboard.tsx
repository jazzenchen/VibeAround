import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
  type Ref,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { DragDropProvider, type DragEndEvent } from "@dnd-kit/react";
import { isSortable } from "@dnd-kit/react/sortable";
import {
  Bot,
  ChevronDown,
  ExternalLink,
  Globe,
  Loader2,
  RotateCw,
  Save,
  SlidersHorizontal,
  Square,
} from "lucide-react";
import { formatErrorMessage } from "@va/client";
import { useI18n } from "@va/i18n";

import { BrandIcon } from "@/components/brand-icon";
import { Button } from "@/components/ui/button";
import { DragHandle, SortableItem } from "./Launch/LaunchBuilderPrimitives";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Switch } from "@/components/ui/switch";
import { StatusBanner } from "@/components/page";
import { apiFetch, DAEMON_PORT, openDashboardUrl } from "@/lib/api";
import { saveSettingsPatch } from "@/lib/settingsPatch";
import { cn } from "@/lib/utils";
import type { AgentRuntime } from "./hooks/useAgentsRuntime";
import type { ChannelRuntime } from "./hooks/useChannelsState";
import type { TunnelRuntime } from "./hooks/useTunnelsState";
import type { Settings as AppSettings } from "./Onboarding/types";
import {
  getLauncherPreferences,
  type AgentSummary,
  listAgents,
  listProfiles,
  type LauncherPreferences,
  setLauncherDefault,
} from "./Launch/api";
import type { ProfileSummary } from "./Launch/types";
import { agentProfileId, profileSupportsAgent } from "./Launch/launchModel";
import {
  basename,
  channelDisplayName,
  channelPresentation,
  shortId,
  tunnelDetail,
  tunnelPresentation,
} from "./status-dashboard/presentation";
import { ServiceIconBadge } from "./status-dashboard/serviceIcon";
import type { RuntimeStateProps, Tone } from "./status-dashboard/types";
import {
  configuredChannelIdsFromSettings,
  defaultAppDefaultForm,
  defaultChannelForm,
  formForAppDefault,
  formForChannel,
  moveChannelOrder,
  orderChannelIds,
  parseRemoteSettings,
  readImChannelOrder,
  resolvedProfileIdForChannel,
  updateRemoteChannelForm,
  writeImChannelOrder,
} from "./remote-dashboard/settings";
import {
  DIRECT_PROFILE,
  FOLLOW_DEFAULT,
  type AppDefaultForm,
  type ChannelDefaultForm,
  type Notice,
  type RemoteSelection,
} from "./remote-dashboard/types";

type RemoteDashboardProps = RuntimeStateProps & {
  onConfigureChannel: (channelId: string) => void;
  onDefaultsChanged?: () => void;
};

export function RemoteDashboard({
  channels,
  tunnels,
  agents,
  onConfigureChannel,
  onDefaultsChanged,
}: RemoteDashboardProps) {
  const { t } = useI18n();
  const [settings, setSettings] = useState<AppSettings>({});
  const persistedSettingsRef = useRef<AppSettings>({});
  const settingsWriteQueueRef = useRef<Promise<void>>(Promise.resolve());
  const [agentDefs, setAgentDefs] = useState<AgentSummary[]>([]);
  const [profiles, setProfiles] = useState<ProfileSummary[]>([]);
  const [prefs, setPrefs] = useState<LauncherPreferences | null>(null);
  const [appDefaultForm, setAppDefaultForm] = useState<AppDefaultForm>(() =>
    defaultAppDefaultForm(),
  );
  const [loadingSettings, setLoadingSettings] = useState(true);
  const [savingChannel, setSavingChannel] = useState<string | null>(null);
  const [savingAppDefault, setSavingAppDefault] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [selection, setSelection] = useState<RemoteSelection | null>(null);

  const remoteSettings = useMemo(() => parseRemoteSettings(settings), [settings]);
  const configuredChannelIds = useMemo(() => {
    const ids = new Set<string>();
    channels.channels.forEach((channel) => ids.add(channel.instance_id));
    configuredChannelIdsFromSettings(settings, remoteSettings).forEach((id) => ids.add(id));
    return orderChannelIds([...ids].filter((id) => id !== "web"), readImChannelOrder(settings));
  }, [channels.channels, remoteSettings.channels, settings]);

  const channelById = useMemo(
    () => new Map(channels.channels.map((channel) => [channel.instance_id, channel])),
    [channels.channels],
  );
  const tunnelById = useMemo(
    () => new Map(tunnels.tunnels.map((tunnel) => [tunnel.provider, tunnel])),
    [tunnels.tunnels],
  );

  useEffect(() => {
    let cancelled = false;
    setLoadingSettings(true);
    setNotice(null);
    void Promise.all([
      invoke<AppSettings>("get_settings"),
      listAgents(),
      listProfiles(),
      getLauncherPreferences(),
    ])
      .then(([loadedSettings, loadedAgents, loadedProfiles, loadedPrefs]) => {
        if (cancelled) return;
        const orderedAgents = orderAgents(loadedAgents);
        setSettings(loadedSettings);
        persistedSettingsRef.current = loadedSettings;
        setAgentDefs(orderedAgents);
        setProfiles(loadedProfiles);
        setPrefs(loadedPrefs);
        setAppDefaultForm(formForAppDefault(loadedPrefs, orderedAgents));
      })
      .catch((error) => {
        if (!cancelled) {
          setNotice({ variant: "error", message: formatErrorMessage(error) });
        }
      })
      .finally(() => {
        if (!cancelled) setLoadingSettings(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (selection) return;
    if (configuredChannelIds.length > 0) {
      setSelection({ kind: "channel", id: configuredChannelIds[0] });
      return;
    }
    if (tunnels.tunnels.length > 0) {
      setSelection({ kind: "tunnel", id: tunnels.tunnels[0].provider });
    }
  }, [configuredChannelIds, selection, tunnels.tunnels]);

  const selectedChannel =
    selection?.kind === "channel" ? channelById.get(selection.id) ?? null : null;
  const selectedTunnel =
    selection?.kind === "tunnel" ? tunnelById.get(selection.id) ?? null : null;
  const selectedChannelId = selection?.kind === "channel" ? selection.id : null;
  const selectedChannelForm = selectedChannelId
    ? formForChannel(remoteSettings, selectedChannelId)
    : defaultChannelForm();

  const updateSelectedChannel = useCallback(
    (patch: Partial<ChannelDefaultForm>) => {
      if (!selectedChannelId) return;
      setSettings((previous) =>
        updateRemoteChannelForm(previous, selectedChannelId, {
          ...formForChannel(parseRemoteSettings(previous), selectedChannelId),
          ...patch,
        }),
      );
      setNotice(null);
    },
    [selectedChannelId],
  );

  const enqueueSettingsSave = useCallback((nextSettings: AppSettings) => {
    const operation = settingsWriteQueueRef.current.then(async () => {
      const saved = await saveSettingsPatch(persistedSettingsRef.current, nextSettings);
      persistedSettingsRef.current = saved.settings;
      setSettings((current) => (current === nextSettings ? saved.settings : current));
      return saved.settings;
    });
    settingsWriteQueueRef.current = operation.then(
      () => undefined,
      () => undefined,
    );
    return operation;
  }, []);

  const persistChannelOrder = useCallback(
    async (nextSettings: AppSettings) => {
      try {
        await enqueueSettingsSave(nextSettings);
      } catch (error) {
        setNotice({ variant: "error", message: formatErrorMessage(error) });
      }
    },
    [enqueueSettingsSave],
  );

  const reorderChannel = useCallback(
    (fromId: string, toId: string) => {
      const nextOrder = moveChannelOrder(configuredChannelIds, fromId, toId);
      const nextSettings = writeImChannelOrder(settings, nextOrder);
      setSettings(nextSettings);
      setNotice(null);
      void persistChannelOrder(nextSettings);
    },
    [configuredChannelIds, persistChannelOrder, settings],
  );

  function handleChannelDragEnd(event: DragEndEvent) {
    if (event.canceled) return;
    const { source } = event.operation;
    if (!isSortable(source) || source.initialIndex === source.index) return;
    const from = configuredChannelIds[source.initialIndex];
    const to = configuredChannelIds[source.index];
    if (from && to) reorderChannel(from, to);
  }

  const saveSelectedChannel = useCallback(async () => {
    if (!selectedChannelId) return;
    setSavingChannel(selectedChannelId);
    setNotice(null);
    try {
      await enqueueSettingsSave(settings);
      const response = await apiFetch("/api/settings/reload", { method: "POST" });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      setNotice({ variant: "success", message: "Remote defaults saved." });
    } catch (error) {
      setNotice({ variant: "error", message: formatErrorMessage(error) });
    } finally {
      setSavingChannel(null);
    }
  }, [enqueueSettingsSave, selectedChannelId, settings]);

  const defaultAgent = prefs?.defaultAgent ?? agentDefs[0]?.id ?? "codex";
  const enabledAgents =
    prefs?.enabledAgents.length
      ? agentDefs.filter((agent) => prefs.enabledAgents.includes(agent.id))
      : agentDefs;
  const selectedAgentId =
    selectedChannelForm.agentId === FOLLOW_DEFAULT
      ? defaultAgent
      : selectedChannelForm.agentId;
  const selectedProfileId =
    selectedChannelForm.profileId === FOLLOW_DEFAULT
      ? prefs
        ? agentProfileId(prefs, selectedAgentId) ?? DIRECT_PROFILE
        : DIRECT_PROFILE
      : selectedChannelForm.profileId;
  const profileOptions = profiles.filter((profile) =>
    prefs ? profileSupportsAgent(profile, selectedAgentId, prefs) : true,
  );
  const appDefaultAgentId = appDefaultForm.agentId || defaultAgent;
  const appDefaultAgentDef = agentDefs.find(
    (agent) => agent.id === appDefaultAgentId,
  );
  const appDefaultProfileLabel =
    appDefaultForm.profileId === DIRECT_PROFILE
      ? t("Direct")
      : profiles.find((profile) => profile.id === appDefaultForm.profileId)?.label ??
        appDefaultForm.profileId;
  const appDefaultProfileOptions = profiles.filter((profile) =>
    prefs ? profileSupportsAgent(profile, appDefaultAgentId, prefs) : true,
  );
  const activeAgentsForChannel = selectedChannelId
    ? agents.agents.filter((agent) => agentRuntimeTouchesChannel(agent, selectedChannelId))
    : [];

  useEffect(() => {
    if (!selectedChannelId) return;
    void agents.refresh();
  }, [selectedChannelId, agents.refresh]);

  const updateAppDefaultAgent = useCallback(
    (agentId: string) => {
      const profileId = prefs ? agentProfileId(prefs, agentId) ?? DIRECT_PROFILE : DIRECT_PROFILE;
      setAppDefaultForm({ agentId, profileId });
      setNotice(null);
    },
    [prefs],
  );

  const saveAppDefault = useCallback(async () => {
    if (!appDefaultForm.agentId) return;
    setSavingAppDefault(true);
    setNotice(null);
    try {
      await setLauncherDefault(
        appDefaultForm.agentId,
        appDefaultForm.profileId === DIRECT_PROFILE ? null : appDefaultForm.profileId,
      );
      const nextPrefs = await getLauncherPreferences();
      setPrefs(nextPrefs);
      setAppDefaultForm(formForAppDefault(nextPrefs, agentDefs));
      onDefaultsChanged?.();
      setNotice({ variant: "success", message: "Default agent saved." });
    } catch (error) {
      setNotice({ variant: "error", message: formatErrorMessage(error) });
    } finally {
      setSavingAppDefault(false);
    }
  }, [agentDefs, appDefaultForm, onDefaultsChanged]);

  return (
    <div className="flex h-full min-h-0 flex-col bg-muted/15">
      <header className="flex h-12 shrink-0 items-center justify-between gap-4 border-b border-border bg-background px-4">
        <div className="flex min-w-0 items-center gap-2">
          <Globe className="h-4 w-4 shrink-0 text-primary" />
          <span className="shrink-0 font-semibold">{t("Remote Access")}</span>
          <span className="min-w-0 truncate text-[11px] text-muted-foreground">
            {t("Configure messaging apps and remote access.")}
          </span>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <DefaultAgentMenu
            form={appDefaultForm}
            agentLabel={appDefaultAgentDef?.display_name ?? appDefaultAgentId}
            profileLabel={appDefaultProfileLabel}
            enabledAgents={enabledAgents}
            profileOptions={appDefaultProfileOptions}
            saving={savingAppDefault}
            onAgentChange={updateAppDefaultAgent}
            onProfileChange={(profileId) => {
              setAppDefaultForm((form) => ({ ...form, profileId }));
              setNotice(null);
            }}
            onSave={() => void saveAppDefault()}
          />
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-7 shrink-0 gap-1.5 px-2 text-[11px] text-primary hover:text-primary"
            title={t("Open Web App")}
            onClick={() =>
              void openDashboardUrl(`http://127.0.0.1:${DAEMON_PORT}/va/`)
            }
          >
            {t("Web App")}
            <ExternalLink className="h-3.5 w-3.5" />
          </Button>
        </div>
      </header>

      {notice && (
        <div className="shrink-0 border-b border-border bg-background px-4 py-2">
          <StatusBanner className="max-w-[960px]" variant={notice.variant}>
            {t(notice.message)}
          </StatusBanner>
        </div>
      )}

      <div className="grid min-h-0 flex-1 grid-cols-[272px_minmax(0,1fr)]">
        <aside className="flex min-h-0 flex-col border-r border-border bg-background/70">
          <div className="min-h-0 flex-1 overflow-y-auto px-2.5 py-2.5 [scrollbar-gutter:stable]">
              <RemoteSidebarSection
                title={t("Messaging apps")}
                count={configuredChannelIds.length}
              >
                {configuredChannelIds.length === 0 ? (
                  <EmptySidebarItem label={t("No messaging apps enabled")} />
                ) : (
                  <DragDropProvider onDragEnd={handleChannelDragEnd}>
                    {configuredChannelIds.map((id, index) => {
                      const channel = channelById.get(id);
                      const presentation = channel
                        ? channelPresentation(channel.status, t)
                        : { label: t("Configured"), tone: "muted" as Tone };
                      const form = formForChannel(remoteSettings, id);
                      const summary = channelDefaultSummary({
                        form,
                        prefs,
                        agentDefs,
                        profiles,
                        defaultAgent,
                        t,
                      });
                      return (
                        <SortableItem
                          key={id}
                          id={id}
                          index={index}
                          disabled={loadingSettings}
                        >
                          {({ dragHandleRef, isDragging }) => (
                            <SidebarButton
                              active={selection?.kind === "channel" && selection.id === id}
                              dragHandleRef={dragHandleRef}
                              isDragging={isDragging}
                              reorderLabel={t("Reorder {{label}}", {
                                label: channelDisplayName(id),
                              })}
                              icon={
                                <ServiceIconBadge
                                  id={id}
                                  kind="channel"
                                  tone={presentation.tone}
                                />
                              }
                              title={channelDisplayName(id)}
                              detail={summary}
                              onClick={() => setSelection({ kind: "channel", id })}
                            />
                          )}
                        </SortableItem>
                      );
                    })}
                  </DragDropProvider>
                )}
              </RemoteSidebarSection>

              <RemoteSidebarSection
                title={t("Remote access")}
                count={tunnels.tunnels.length}
                className="mt-5"
              >
                {tunnels.tunnels.length === 0 ? (
                  <EmptySidebarItem label={t("No tunnel running")} />
                ) : (
                  tunnels.tunnels.map((tunnel) => {
                    const presentation = tunnelPresentation(tunnel.status, t);
                    return (
                      <SidebarButton
                        key={tunnel.provider}
                        active={
                          selection?.kind === "tunnel" &&
                          selection.id === tunnel.provider
                        }
                        icon={
                          <ServiceIconBadge
                            id={tunnel.provider}
                            kind="tunnel"
                            tone={presentation.tone}
                          />
                        }
                        title={t("{{provider}} tunnel", {
                          provider: capitalize(tunnel.provider),
                        })}
                        detail={tunnel.url ?? tunnelDetail(tunnel.status) ?? ""}
                        onClick={() =>
                          setSelection({ kind: "tunnel", id: tunnel.provider })
                        }
                      />
                    );
                  })
                )}
              </RemoteSidebarSection>
            </div>
          </aside>

          <main className="min-h-0 overflow-y-auto px-6 py-5 [scrollbar-gutter:stable]">
            {loadingSettings ? (
              <div className="flex h-48 items-center justify-center text-xs text-muted-foreground">
                <Loader2 className="mr-2 h-3.5 w-3.5 animate-spin" />
                {t("Loading…")}
              </div>
            ) : selectedChannelId ? (
              <div className="mx-auto max-w-[960px]">
                <ChannelRemoteDetail
                  channelId={selectedChannelId}
                  channel={selectedChannel}
                  form={selectedChannelForm}
                  enabledAgents={enabledAgents}
                  selectedAgentId={selectedAgentId}
                  selectedProfileId={selectedProfileId}
                  profileOptions={profileOptions}
                  activeAgents={activeAgentsForChannel}
                  agentsLoading={agents.loading}
                  agentsEverLoaded={agents.everLoaded}
                  agentsError={agents.error}
                  pluginDir={selectedChannel?.plugin_dir ?? null}
                  saving={savingChannel === selectedChannelId}
                  onAgentChange={(agentId) =>
                    updateSelectedChannel({ agentId, profileId: FOLLOW_DEFAULT })
                  }
                  onProfileChange={(profileId) =>
                    updateSelectedChannel({ profileId })
                  }
                  onSave={() => void saveSelectedChannel()}
                  onConfigure={() =>
                    onConfigureChannel(selectedChannel?.kind ?? selectedChannelId)
                  }
                  onRefreshAgents={() => void agents.refresh()}
                  onStart={() => channels.start(selectedChannelId)}
                  onStop={() => channels.stop(selectedChannelId)}
                  onRestart={() => channels.restart(selectedChannelId)}
                />
              </div>
            ) : selectedTunnel ? (
              <div className="mx-auto max-w-[960px]">
                <TunnelRemoteDetail
                  tunnel={selectedTunnel}
                  onKill={() => tunnels.kill(selectedTunnel.provider)}
                />
              </div>
            ) : (
              <div className="mx-auto max-w-[960px] rounded-md border border-dashed border-border px-4 py-10 text-center text-sm text-muted-foreground">
                {t("Select a remote entry.")}
              </div>
            )}
          </main>
        </div>
    </div>
  );
}

function DefaultAgentMenu({
  form,
  agentLabel,
  profileLabel,
  enabledAgents,
  profileOptions,
  saving,
  onAgentChange,
  onProfileChange,
  onSave,
}: {
  form: AppDefaultForm;
  agentLabel: string;
  profileLabel: string;
  enabledAgents: AgentSummary[];
  profileOptions: ProfileSummary[];
  saving: boolean;
  onAgentChange: (agentId: string) => void;
  onProfileChange: (profileId: string) => void;
  onSave: () => void;
}) {
  const { t } = useI18n();
  const agentOptions = enabledAgents.map((agent) => ({
    value: agent.id,
    label: agent.display_name,
    icon: (
      <BrandIcon
        kind="cli"
        id={agent.id}
        label={agent.display_name}
        className="h-5 w-5"
      />
    ),
  }));
  const profileOptionsForDropdown = [
    { value: DIRECT_PROFILE, label: t("Direct") },
    ...profileOptions.map((profile) => ({
      value: profile.id,
      label: profile.label,
    })),
  ];

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="h-7 w-auto justify-start gap-1.5 px-2 text-[11px] font-normal"
          title={`${t("Default agent")}: ${agentLabel} · ${profileLabel}`}
        >
          <BrandIcon
            kind="cli"
            id={form.agentId}
            label={agentLabel}
            className="h-4 w-4"
          />
          <span className="shrink-0 font-medium text-foreground">
            {t("Default agent")}
          </span>
          <span className="whitespace-nowrap text-muted-foreground">
            {agentLabel} · {profileLabel}
          </span>
          <ChevronDown className="h-3.5 w-3.5 shrink-0 opacity-60" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-72">
        <DropdownMenuLabel className="px-2 py-1.5 text-[10px] font-medium uppercase text-muted-foreground/60">
          {t("Default agent")}
        </DropdownMenuLabel>
        <DropdownMenuRadioGroup value={form.agentId} onValueChange={onAgentChange}>
          {agentOptions.map((option) => (
            <DropdownMenuRadioItem
              key={option.value}
              value={option.value}
              className="items-center text-xs"
              onSelect={(event) => event.preventDefault()}
            >
              <span className="flex h-5 w-5 shrink-0 items-center justify-center">
                {option.icon}
              </span>
              <span className="min-w-0 truncate">{option.label}</span>
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
        <DropdownMenuSeparator />
        <DropdownMenuLabel className="px-2 py-1.5 text-[10px] font-medium uppercase text-muted-foreground/60">
          {t("Profile")}
        </DropdownMenuLabel>
        <DropdownMenuRadioGroup
          value={form.profileId}
          onValueChange={onProfileChange}
        >
          {profileOptionsForDropdown.map((option) => (
            <DropdownMenuRadioItem
              key={option.value}
              value={option.value}
              className="text-xs"
              onSelect={(event) => event.preventDefault()}
            >
              <span className="min-w-0 truncate">{option.label}</span>
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
        <DropdownMenuSeparator />
        <div className="flex justify-end px-1 py-1">
          <Button
            type="button"
            size="sm"
            className="h-7 shrink-0 gap-1.5 px-2 text-[11px]"
            disabled={saving}
            onClick={onSave}
          >
            {saving ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Save className="h-3.5 w-3.5" />
            )}
            {saving ? t("Saving…") : t("Save")}
          </Button>
        </div>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function ChannelRemoteDetail({
  channelId,
  channel,
  form,
  enabledAgents,
  selectedAgentId,
  selectedProfileId,
  profileOptions,
  activeAgents,
  agentsLoading,
  agentsEverLoaded,
  agentsError,
  pluginDir,
  saving,
  onAgentChange,
  onProfileChange,
  onSave,
  onConfigure,
  onRefreshAgents,
  onStart,
  onStop,
  onRestart,
}: {
  channelId: string;
  channel: ChannelRuntime | null;
  form: ChannelDefaultForm;
  enabledAgents: AgentSummary[];
  selectedAgentId: string;
  selectedProfileId: string;
  profileOptions: ProfileSummary[];
  activeAgents: AgentRuntime[];
  agentsLoading: boolean;
  agentsEverLoaded: boolean;
  agentsError: string | null;
  pluginDir: string | null;
  saving: boolean;
  onAgentChange: (agentId: string) => void;
  onProfileChange: (profileId: string) => void;
  onSave: () => void;
  onConfigure: () => void;
  onRefreshAgents: () => unknown;
  onStart: () => unknown;
  onStop: () => unknown;
  onRestart: () => unknown;
}) {
  const { t } = useI18n();
  const presentation = channel
    ? channelPresentation(channel.status, t)
    : { label: t("Configured"), tone: "muted" as Tone };
  const running = channel?.status === "running" || channel?.status === "spawning";
  const agentOptions = [
    { value: FOLLOW_DEFAULT, label: t("Follow app default") },
    ...enabledAgents.map((agent) => ({
      value: agent.id,
      label: agent.display_name,
      icon: (
        <BrandIcon
          kind="cli"
          id={agent.id}
          label={agent.display_name}
          className="h-5 w-5"
        />
      ),
    })),
  ];
  const profileDropdownOptions = [
    { value: FOLLOW_DEFAULT, label: t("Follow agent default") },
    { value: DIRECT_PROFILE, label: t("Direct") },
    ...profileOptions.map((profile) => ({
      value: profile.id,
      label: profile.label,
    })),
  ];
  return (
    <div className="grid gap-4">
      <section className="flex items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-2.5">
          <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-md border border-border bg-card">
            <ServiceIconBadge
              id={channelId}
              kind="channel"
              tone={presentation.tone}
              showStatus={false}
            />
          </span>
          <div className="min-w-0">
            <div className="flex min-w-0 flex-wrap items-center gap-1.5">
              <h1 className="truncate text-lg font-semibold leading-tight">
                {channelDisplayName(channelId)}
              </h1>
              <span
                className={cn(
                  "rounded-md border px-1.5 py-0.5 text-[11px]",
                  toneBadgeClass(presentation.tone),
                )}
              >
                {presentation.label}
              </span>
            </div>
            <div className="mt-0.5 text-[11px] text-muted-foreground">
              {channel?.version ? `v${channel.version}` : t("Plugin status")}
            </div>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-2 text-[11px] text-muted-foreground">
          <Button
            type="button"
            variant="outline"
            size="icon-xs"
            className="h-7 w-7"
            title={t("Configure")}
            aria-label={t("Configure")}
            onClick={onConfigure}
          >
            <SlidersHorizontal className="h-3.5 w-3.5" />
          </Button>
          <span className="font-medium text-primary">
            {running ? t("Enabled") : t("Disabled")}
          </span>
          <Switch
            checked={running}
            onCheckedChange={(checked) => {
              if (checked) onStart();
              else onStop();
            }}
            size="sm"
            aria-label={t("Toggle channel")}
          />
        </div>
      </section>

      <section className="rounded-md border border-border bg-card px-3 py-3">
        <div className="mb-3 flex items-center justify-between gap-3">
          <div className="flex min-w-0 items-center gap-2 text-xs font-semibold">
            <Bot className="h-3.5 w-3.5 text-primary" />
            {t("Agent configuration")}
          </div>
          <div className="flex shrink-0 items-center gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="h-7 gap-1.5 px-2 text-[11px]"
              onClick={onRestart}
            >
              <RotateCw className="h-3.5 w-3.5" />
              {t("Restart")}
            </Button>
            <Button
              type="button"
              size="sm"
              className="h-7 gap-1.5 px-2 text-[11px]"
              disabled={saving}
              onClick={onSave}
            >
              {saving ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Save className="h-3.5 w-3.5" />
              )}
              {saving ? t("Saving…") : t("Save")}
            </Button>
          </div>
        </div>
        <div className="grid gap-2 lg:grid-cols-2">
          <SelectField label={t("Agent")}>
            <RemoteDropdownField
              label={t("Agent")}
              value={form.agentId}
              options={agentOptions}
              onValueChange={onAgentChange}
            />
          </SelectField>
          <SelectField label={t("Profile")}>
            <RemoteDropdownField
              label={t("Profile")}
              value={form.profileId}
              options={profileDropdownOptions}
              onValueChange={onProfileChange}
            />
          </SelectField>
        </div>
        <div className="mt-3 border-t border-border/70 pt-2.5">
          <div className="grid gap-x-3 gap-y-1 text-[11px] text-muted-foreground sm:grid-cols-[auto_minmax(0,1fr)]">
            <span className="font-medium text-foreground">
              {t("Plugin directory")}
            </span>
            <span className="min-w-0 truncate font-mono text-foreground">
              {pluginDir ?? t("Unavailable")}
            </span>
          </div>
        </div>
      </section>

      <div className="grid gap-4 xl:grid-cols-2">
        <section className="rounded-md border border-border bg-card px-3 py-3">
          <div className="mb-2 text-xs font-semibold">{t("Current setting")}</div>
          <div className="space-y-2">
            <KeyValue label={t("Agent")} value={selectedAgentId} />
            <KeyValue label={t("Profile")} value={selectedProfileId} />
            {channel?.reason && <KeyValue label={t("Reason")} value={channel.reason} danger />}
          </div>
        </section>

        <section className="rounded-md border border-border bg-card px-3 py-3">
          <div className="mb-2 flex items-center justify-between gap-2">
            <div className="text-xs font-semibold">{t("Active sessions")}</div>
            <Button
              type="button"
              variant="ghost"
              size="icon-xs"
              className="h-6 w-6"
              title={t("Refresh")}
              aria-label={t("Refresh")}
              onClick={() => void onRefreshAgents()}
            >
              <RotateCw
                className={cn("h-3.5 w-3.5", agentsLoading && "animate-spin")}
              />
            </Button>
          </div>
          {!agentsEverLoaded && agentsLoading ? (
            <div className="rounded-md border border-dashed border-border px-3 py-6 text-center text-xs text-muted-foreground">
              {t("Loading…")}
            </div>
          ) : agentsError && activeAgents.length === 0 ? (
            <div className="rounded-md border border-dashed border-destructive/40 px-3 py-6 text-center text-xs text-destructive">
              {agentsError}
            </div>
          ) : activeAgents.length === 0 ? (
            <div className="rounded-md border border-dashed border-border px-3 py-6 text-center text-xs text-muted-foreground">
              {t("No active agent session for this channel.")}
            </div>
          ) : (
            <div className="grid gap-1.5">
              {activeAgents.map((agent) => (
                <div
                  key={agent.route_key}
                  className="flex min-h-[42px] items-center justify-between gap-3 rounded-md border border-border/70 px-2 py-1.5"
                >
                  <div className="flex min-w-0 items-center gap-2">
                    <BrandIcon
                      kind="cli"
                      id={agent.cli_kind ?? selectedAgentId}
                      label={agent.agent_title ?? agent.agent_name ?? agent.cli_kind ?? ""}
                      className="h-6 w-6"
                    />
                    <div className="min-w-0">
                      <div className="truncate text-xs font-medium">
                        {agent.agent_title ?? agent.agent_name ?? agent.cli_kind}
                      </div>
                      <div className="truncate text-[11px] text-muted-foreground">
                        {agent.session_id ? shortId(agent.session_id) : t("No session yet")} ·{" "}
                        {agent.workspace ? basename(agent.workspace) : t("Workspace")}
                      </div>
                    </div>
                  </div>
                  <span className="text-[11px] text-muted-foreground">
                    {agent.busy ? t("Busy") : t("Idle")}
                  </span>
                </div>
              ))}
            </div>
          )}
        </section>
      </div>
    </div>
  );
}

function TunnelRemoteDetail({
  tunnel,
  onKill,
}: {
  tunnel: TunnelRuntime;
  onKill: () => unknown;
}) {
  const { t } = useI18n();
  const presentation = tunnelPresentation(tunnel.status, t);
  const detail = tunnelDetail(tunnel.status);
  return (
    <div className="grid gap-4">
      <section className="flex items-start justify-between gap-4">
        <div className="flex min-w-0 items-start gap-3">
          <span className="flex h-10 w-10 shrink-0 items-center justify-center rounded-md border border-border bg-card">
            <ServiceIconBadge
              id={tunnel.provider}
              kind="tunnel"
              tone={presentation.tone}
              showStatus={false}
            />
          </span>
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2">
              <h1 className="truncate text-xl font-semibold">
                {t("{{provider}} tunnel", { provider: capitalize(tunnel.provider) })}
              </h1>
              <span
                className={cn(
                  "rounded-md border px-1.5 py-0.5 text-[11px]",
                  toneBadgeClass(presentation.tone),
                )}
              >
                {presentation.label}
              </span>
            </div>
            <div className="mt-1 text-xs text-muted-foreground">
              {tunnel.url ?? detail ?? t("No public URL")}
            </div>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {tunnel.url && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="h-7 gap-1.5 px-2 text-[11px]"
              onClick={() => void openDashboardUrl(tunnel.url!)}
            >
              <ExternalLink className="h-3.5 w-3.5" />
              {t("Open")}
            </Button>
          )}
          {tunnel.status.state === "running" && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="h-7 gap-1.5 px-2 text-[11px]"
              onClick={onKill}
            >
              <Square className="h-3.5 w-3.5" />
              {t("Stop")}
            </Button>
          )}
        </div>
      </section>

      <section className="rounded-md border border-border bg-card px-3 py-3">
        <div className="mb-2 flex items-center gap-2 text-xs font-semibold">
          <Globe className="h-3.5 w-3.5 text-primary" />
          {t("Tunnel information")}
        </div>
        <div className="space-y-2">
          <KeyValue label={t("Provider")} value={tunnel.provider} />
          <KeyValue label={t("Status")} value={presentation.label} />
          <KeyValue label={t("Public URL")} value={tunnel.url ?? t("Unavailable")} />
          <KeyValue label={t("Uptime")} value={`${tunnel.uptime_secs}s`} />
          {detail && <KeyValue label={t("Reason")} value={detail} danger />}
        </div>
      </section>
    </div>
  );
}

function RemoteSidebarSection({
  title,
  count,
  className,
  children,
}: {
  title: string;
  count: number;
  className?: string;
  children: ReactNode;
}) {
  return (
    <section className={cn("mb-4", className)}>
      <div className="mb-2 flex items-center justify-between px-1 text-[11px] font-medium text-muted-foreground">
        <span>{title}</span>
        <span>{count}</span>
      </div>
      <div className="grid gap-1.5">{children}</div>
    </section>
  );
}

function SidebarButton({
  active,
  dragHandleRef,
  isDragging = false,
  reorderLabel,
  icon,
  title,
  detail,
  onClick,
}: {
  active: boolean;
  dragHandleRef?: Ref<HTMLSpanElement>;
  isDragging?: boolean;
  reorderLabel?: string;
  icon: ReactNode;
  title: string;
  detail: string;
  onClick: () => void;
}) {
  return (
    <div
      role="button"
      tabIndex={0}
      className={cn(
        "flex min-h-[46px] w-full items-center gap-2 rounded-md border px-2 text-left transition-colors",
        active
          ? "border-primary bg-card shadow-[inset_3px_0_0_hsl(var(--primary))]"
          : "border-transparent hover:border-border hover:bg-card",
        isDragging && "opacity-55",
      )}
      onClick={onClick}
      onKeyDown={(event) => {
        if (event.key !== "Enter" && event.key !== " ") return;
        event.preventDefault();
        onClick();
      }}
    >
      {reorderLabel && (
        <DragHandle
          label={reorderLabel}
          dragHandleRef={dragHandleRef}
        />
      )}
      {icon}
      <span className="min-w-0 flex-1">
        <span className="block">
          <span className="truncate text-xs font-semibold">{title}</span>
        </span>
        <span className="mt-0.5 block truncate text-[11px] text-muted-foreground">
          {detail}
        </span>
      </span>
    </div>
  );
}

function EmptySidebarItem({ label }: { label: string }) {
  return (
    <div className="rounded-md border border-dashed border-border px-3 py-4 text-center text-xs text-muted-foreground">
      {label}
    </div>
  );
}

type RemoteDropdownOption = {
  value: string;
  label: string;
  detail?: string;
  icon?: ReactNode;
  disabled?: boolean;
};

function RemoteDropdownField({
  label,
  value,
  options,
  onValueChange,
}: {
  label: string;
  value: string;
  options: RemoteDropdownOption[];
  onValueChange: (value: string) => void;
}) {
  const selected = options.find((option) => option.value === value) ?? options[0];

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="h-8 w-full min-w-0 justify-start gap-1.5 px-2 text-xs font-normal"
          title={selected?.detail ?? selected?.label}
        >
          {selected?.icon && (
            <span className="flex h-5 w-5 shrink-0 items-center justify-center">
              {selected.icon}
            </span>
          )}
          <span className="min-w-0 flex-1 truncate text-left">
            {selected?.label ?? label}
          </span>
          <ChevronDown className="h-3.5 w-3.5 shrink-0 opacity-60" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="start"
        className="max-h-72 w-[var(--radix-dropdown-menu-trigger-width)] min-w-56"
      >
        <DropdownMenuLabel className="px-2 py-1.5 text-[10px] font-medium uppercase text-muted-foreground/60">
          {label}
        </DropdownMenuLabel>
        <DropdownMenuRadioGroup value={value} onValueChange={onValueChange}>
          {options.map((option) => (
            <DropdownMenuRadioItem
              key={option.value}
              value={option.value}
              disabled={option.disabled}
              className="items-start text-xs"
            >
              {option.icon && (
                <span className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center">
                  {option.icon}
                </span>
              )}
              <span className="min-w-0 flex-1">
                <span className="block truncate">{option.label}</span>
                {option.detail && (
                  <span className="block truncate text-[10px] text-muted-foreground/60">
                    {option.detail}
                  </span>
                )}
              </span>
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function SelectField({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <label className="block">
      <span className="mb-1 block text-[11px] font-medium text-muted-foreground">
        {label}
      </span>
      {children}
    </label>
  );
}

function KeyValue({
  label,
  value,
  danger,
}: {
  label: string;
  value: ReactNode;
  danger?: boolean;
}) {
  return (
    <div className="grid grid-cols-[104px_1fr] items-start gap-3 border-b border-border/70 pb-1.5 last:border-b-0 last:pb-0">
      <div className="text-[11px] text-muted-foreground">{label}</div>
      <div className={cn("min-w-0 break-words text-xs", danger && "text-destructive")}>
        {value}
      </div>
    </div>
  );
}

function channelDefaultSummary({
  form,
  prefs,
  agentDefs,
  profiles,
  defaultAgent,
  t,
}: {
  form: ChannelDefaultForm;
  prefs: LauncherPreferences | null;
  agentDefs: AgentSummary[];
  profiles: ProfileSummary[];
  defaultAgent: string;
  t: ReturnType<typeof useI18n>["t"];
}) {
  const agentId = form.agentId === FOLLOW_DEFAULT ? defaultAgent : form.agentId;
  const agent = agentDefs.find((item) => item.id === agentId);
  const profileId = resolvedProfileIdForChannel(form, prefs, agentId);
  const profile =
    profileId && profileId !== DIRECT_PROFILE
      ? profiles.find((item) => item.id === profileId)?.label ?? profileId
      : t("Direct");
  return `${agent?.display_name ?? agentId} · ${profile}`;
}

function agentRuntimeTouchesChannel(agent: AgentRuntime, channelId: string) {
  if (agent.channel_kind === channelId) return true;
  return agent.attached_routes.some((route) => route.channel_kind === channelId);
}

function orderAgents(agents: AgentSummary[]) {
  const order = ["claude", "codex", "pi", "gemini", "opencode", "cursor", "kiro", "qwen-code"];
  return [...agents].sort((a, b) => {
    const ai = order.indexOf(a.id);
    const bi = order.indexOf(b.id);
    if (ai >= 0 && bi >= 0) return ai - bi;
    if (ai >= 0) return -1;
    if (bi >= 0) return 1;
    return a.display_name.localeCompare(b.display_name);
  });
}

function toneBadgeClass(tone: Tone) {
  switch (tone) {
    case "good":
      return "border-emerald-500/30 bg-emerald-50 text-emerald-700";
    case "busy":
      return "border-blue-500/30 bg-blue-50 text-blue-700";
    case "warning":
      return "border-amber-500/30 bg-amber-50 text-amber-700";
    case "danger":
      return "border-destructive/30 bg-destructive/10 text-destructive";
    case "muted":
      return "border-border bg-muted/30 text-muted-foreground";
  }
}

function capitalize(value: string): string {
  return value.length > 0 ? value[0].toUpperCase() + value.slice(1) : value;
}
