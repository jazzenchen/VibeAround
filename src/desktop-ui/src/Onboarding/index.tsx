import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  AlertCircle,
  CheckCircle2,
  Circle,
  Download,
  Globe,
  Loader2,
  MessageSquare,
  RefreshCw,
  Rocket,
  Settings2,
  TerminalSquare,
  Wrench,
} from "lucide-react";
import { useI18n } from "@va/i18n";

import { LanguageMenu } from "@/components/LanguageMenu";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

import { StepAgents } from "./components/StepAgents";
import { StepChannels } from "./components/StepChannels";
import { StepTunnel } from "./components/StepTunnel";
import { useChannelAuth } from "./hooks/useChannelAuth";
import { useStartkitFlow } from "./hooks/useStartkitFlow";
import { buildSettings } from "./lib/buildSettings";
import {
  createProfile,
  deleteProfile,
  listCatalog,
  listProfiles,
  upsertProfile,
} from "../Launch/api";
import { ProfileFormDialog } from "../Launch/ProfileFormDialog";
import type { ProfileFormSubmit } from "../Launch/ProfileFormDialog";
import type { CatalogEntry, ProfileSummary } from "../Launch/types";
import type {
  AgentSummary,
  ChannelVerboseConfig,
  DiscoveredChannelPlugin,
  PluginRegistryEntry,
  Settings,
  StartkitChoices,
  StartkitItemReport,
  StartkitManifestSummary,
  StartkitStatus,
  TunnelSummary,
} from "./types";
import type { AgentId, TunnelProvider } from "./constants";

const DEFAULT_ENABLED_AGENT_IDS = new Set<AgentId>(["claude", "codex"]);
const AGENT_DISPLAY_ORDER = [
  "claude",
  "codex",
  "pi",
  "gemini",
  "opencode",
  "cursor",
  "kiro",
  "qwen-code",
];
const GROUP_ORDER = ["computer", "agents", "remote", "messaging"];

function orderAgents(agentDefs: AgentSummary[]): AgentSummary[] {
  const rank = new Map(AGENT_DISPLAY_ORDER.map((id, index) => [id, index]));
  return [...agentDefs].sort(
    (a, b) => (rank.get(a.id) ?? 999) - (rank.get(b.id) ?? 999),
  );
}

export default function Onboarding() {
  const { t } = useI18n();
  const isMacTitlebar =
    typeof navigator !== "undefined" && /Mac/.test(navigator.platform);

  const [settings, setSettings] = useState<Settings>({});
  const [loaded, setLoaded] = useState(false);
  const [manifest, setManifest] = useState<StartkitManifestSummary | null>(
    null,
  );
  const [catalog, setCatalog] = useState<CatalogEntry[]>([]);
  const [profiles, setProfiles] = useState<ProfileSummary[]>([]);
  const [profileEditorOpen, setProfileEditorOpen] = useState(false);
  const [agents, setAgents] = useState<AgentSummary[]>([]);
  const [tunnels, setTunnels] = useState<TunnelSummary[]>([]);
  const [pluginRegistry, setPluginRegistry] = useState<PluginRegistryEntry[]>(
    [],
  );
  const [discoveredPlugins, setDiscoveredPlugins] = useState<
    DiscoveredChannelPlugin[]
  >([]);

  const [downloadSource, setDownloadSource] = useState("global");
  const [toolchainMode, setToolchainMode] = useState<
    "auto" | "managed" | "system"
  >("auto");
  const [enabledAgents, setEnabledAgents] = useState<Set<AgentId>>(new Set());
  const [enabledChannels, setEnabledChannels] = useState<Set<string>>(
    new Set(),
  );
  const [channelConfigs, setChannelConfigs] = useState<
    Record<string, Record<string, string>>
  >({});
  const [channelVerbose, setChannelVerbose] = useState<
    Record<string, ChannelVerboseConfig>
  >({});
  const [installingPlugins, setInstallingPlugins] = useState<Set<string>>(
    new Set(),
  );

  const [tunnelProvider, setTunnelProvider] =
    useState<TunnelProvider>("cloudflare");
  const [ngrokToken, setNgrokToken] = useState("");
  const [ngrokDomain, setNgrokDomain] = useState("");
  const [cfToken, setCfToken] = useState("");
  const [cfHostname, setCfHostname] = useState("");

  const startkit = useStartkitFlow();

  useEffect(() => {
    Promise.all([
      invoke<Settings>("get_settings"),
      invoke<DiscoveredChannelPlugin[]>("list_channel_plugins"),
      invoke<AgentSummary[]>("list_agents"),
      invoke<TunnelSummary[]>("list_tunnels"),
      invoke<PluginRegistryEntry[]>("list_plugin_registry"),
      invoke<StartkitManifestSummary>("startkit_manifest"),
      listCatalog(),
      listProfiles(),
    ])
      .then(
        ([
          loadedSettings,
          plugins,
          agentDefs,
          tunnelDefs,
          pluginDefs,
          startkitManifest,
          catalogDefs,
          profileDefs,
        ]) => {
          const orderedAgents = orderAgents(agentDefs);
          setSettings(loadedSettings);
          setDiscoveredPlugins(plugins);
          setAgents(orderedAgents);
          setTunnels(tunnelDefs);
          setPluginRegistry(pluginDefs);
          setManifest(startkitManifest);
          if (loadedSettings.startkit?.source) {
            setDownloadSource(loadedSettings.startkit.source);
          }
          if (
            loadedSettings.startkit?.toolchain_mode === "auto" ||
            loadedSettings.startkit?.toolchain_mode === "managed" ||
            loadedSettings.startkit?.toolchain_mode === "system"
          ) {
            setToolchainMode(loadedSettings.startkit.toolchain_mode);
          }
          setCatalog(catalogDefs);
          setProfiles(profileDefs);

          const registryPluginIds = new Set(pluginDefs.map((p) => p.id));

          if (Array.isArray(loadedSettings.enabled_agents)) {
            setEnabledAgents(
              new Set(loadedSettings.enabled_agents as AgentId[]),
            );
          } else {
            setEnabledAgents(
              new Set(
                orderedAgents
                  .map((agent) => agent.id)
                  .filter((id) => DEFAULT_ENABLED_AGENT_IDS.has(id)),
              ),
            );
          }

          const channels = loadedSettings.channels ?? {};
          const enabled = new Set<string>();
          const configs: Record<string, Record<string, string>> = {};
          const verbose: Record<string, ChannelVerboseConfig> = {};
          for (const [id, channelConfig] of Object.entries(channels)) {
            if (!registryPluginIds.has(id)) continue;
            enabled.add(id);
            const configMap: Record<string, string> = {};
            for (const [key, value] of Object.entries(channelConfig)) {
              if (key !== "verbose" && typeof value === "string") {
                configMap[key] = value;
              }
            }
            configs[id] = configMap;
            verbose[id] = parseChannelVerbose(channelConfig.verbose);
          }
          setEnabledChannels(enabled);
          setChannelConfigs(configs);
          setChannelVerbose(verbose);

          const provider = loadedSettings.tunnel?.provider;
          if (
            provider === "none" ||
            provider === "cloudflare" ||
            provider === "ngrok" ||
            provider === "localtunnel"
          ) {
            setTunnelProvider(provider);
          }
          if (loadedSettings.tunnel?.ngrok?.auth_token)
            setNgrokToken(loadedSettings.tunnel.ngrok.auth_token);
          if (loadedSettings.tunnel?.ngrok?.domain)
            setNgrokDomain(loadedSettings.tunnel.ngrok.domain);
          if (loadedSettings.tunnel?.cloudflare?.tunnel_token)
            setCfToken(loadedSettings.tunnel.cloudflare.tunnel_token);
          if (loadedSettings.tunnel?.cloudflare?.hostname)
            setCfHostname(loadedSettings.tunnel.cloudflare.hostname);

          setLoaded(true);
        },
      )
      .catch((error) => {
        console.error("failed to load onboarding data", error);
        setLoaded(true);
      });
  }, []);

  const registryPluginIds = useMemo(
    () => new Set(pluginRegistry.map((plugin) => plugin.id)),
    [pluginRegistry],
  );

  const choices: StartkitChoices = useMemo(
    () => ({
      agents: Array.from(enabledAgents),
      tunnel: tunnelProvider,
      channels: Array.from(enabledChannels),
      source: downloadSource,
      toolchainMode,
    }),
    [enabledAgents, tunnelProvider, enabledChannels, downloadSource, toolchainMode],
  );

  const finalSettings = useMemo(
    () => {
      const built = buildSettings({
        settings,
        configureAgents: true,
        configureChannels: true,
        configureTunnel: true,
        enabledAgents,
        enabledChannels,
        registryPluginIds,
        channelConfigs,
        channelVerbose,
        discoveredPlugins,
        tunnelProvider,
        ngrokToken,
        ngrokDomain,
        cfToken,
        cfHostname,
      });
      return {
        ...built,
        startkit: {
          ...(typeof built.startkit === "object" && built.startkit !== null
            ? built.startkit
            : {}),
          source: downloadSource,
          toolchain_mode: toolchainMode,
        },
      };
    },
    [
      settings,
      enabledAgents,
      enabledChannels,
      registryPluginIds,
      channelConfigs,
      channelVerbose,
      discoveredPlugins,
      tunnelProvider,
      ngrokToken,
      ngrokDomain,
      cfToken,
      cfHostname,
      downloadSource,
      toolchainMode,
    ],
  );

  useEffect(() => {
    if (!loaded) return;
    void startkit.refreshPlan(choices);
  }, [loaded, choices, startkit.refreshPlan]);

  const toggleAgent = useCallback((id: AgentId) => {
    setEnabledAgents((previous) => {
      const next = new Set(previous);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const toggleChannel = useCallback((pluginId: string, enabled: boolean) => {
    setEnabledChannels((prev) => {
      const next = new Set(prev);
      if (enabled) next.add(pluginId);
      else next.delete(pluginId);
      return next;
    });
    if (enabled) {
      setChannelVerbose((prev) =>
        prev[pluginId] ? prev : { ...prev, [pluginId]: defaultChannelVerbose() },
      );
    }
  }, []);

  const updateChannelConfig = useCallback(
    (pluginId: string, key: string, value: string) => {
      setChannelConfigs((prev) => ({
        ...prev,
        [pluginId]: { ...(prev[pluginId] ?? {}), [key]: value },
      }));
    },
    [],
  );

  const updateChannelVerbose = useCallback(
    (
      pluginId: string,
      key: keyof ChannelVerboseConfig,
      value: boolean,
    ) => {
      setChannelVerbose((prev) => ({
        ...prev,
        [pluginId]: {
          ...(prev[pluginId] ?? defaultChannelVerbose()),
          [key]: value,
        },
      }));
    },
    [],
  );

  const installPlugin = useCallback(
    async (pluginId: string, githubUrl: string) => {
      setInstallingPlugins((prev) => new Set(prev).add(pluginId));
      try {
        await invoke("install_plugin", { request: { pluginId, githubUrl } });
        const plugins = await invoke<DiscoveredChannelPlugin[]>(
          "list_channel_plugins",
        );
        setDiscoveredPlugins(plugins);
      } catch (error) {
        console.error(`Failed to install plugin ${pluginId}:`, error);
      } finally {
        setInstallingPlugins((prev) => {
          const next = new Set(prev);
          next.delete(pluginId);
          return next;
        });
      }
    },
    [],
  );

  const { authStates, startAuth, cancelAuth } = useChannelAuth({
    active: true,
    discoveredPlugins,
    channelConfigs,
    onConfigChange: updateChannelConfig,
  });

  const handleSaveProfile = useCallback(async (submit: ProfileFormSubmit) => {
    if (submit.type === "create") {
      await createProfile(submit.draft);
    } else {
      await upsertProfile(submit.profile);
    }
    const nextProfiles = await listProfiles();
    setProfiles(nextProfiles);
  }, []);

  const handleDeleteProfile = useCallback(
    async (id: string) => {
      const profile = profiles.find((item) => item.id === id);
      if (
        profile &&
        !window.confirm(
          t('Delete profile "{{label}}"?', { label: profile.label }),
        )
      )
        return;
      await deleteProfile(id);
      const nextProfiles = await listProfiles();
      setProfiles(nextProfiles);
    },
    [profiles, t],
  );

  const groupedReports = useMemo(
    () => groupReports(startkit.plan?.items ?? [], startkit.reportById),
    [startkit.plan, startkit.reportById],
  );

  const needsAttention = startkit.reports.some((report) =>
    ["missing", "outdated", "broken", "needs_config", "blocked", "error"].includes(
      report.status,
    ),
  );
  const hasScanned = startkit.reports.some((report) => report.status !== "pending");
  const canFinish = startkit.complete || (hasScanned && !needsAttention);

  if (!loaded) {
    return (
      <div className="flex h-full items-center justify-center">
        <span className="text-sm text-muted-foreground animate-pulse">
          {t("Loading…")}
        </span>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col bg-background">
      <header
        className={cn(
          "relative flex h-12 items-center gap-3 border-b border-border pr-6",
          isMacTitlebar ? "pl-[82px]" : "pl-6",
        )}
      >
        <div
          data-tauri-drag-region
          aria-hidden="true"
          className="absolute inset-0 z-0"
        />
        <div className="relative z-10 flex shrink-0 items-baseline gap-1.5 whitespace-nowrap">
          <span className="text-[13px] font-semibold text-foreground">
            VibeAround
          </span>
          <span className="font-mono text-[10px] text-muted-foreground/60">
            @{__APP_VERSION_LABEL__}
          </span>
        </div>
        <div className="relative z-10 min-w-0 flex-1">
          <div className="truncate text-xs font-medium text-muted-foreground">
            {t("Startkit prepares this computer for local agents, remote access, and messaging.")}
          </div>
        </div>
        <div className="relative z-10">
          <LanguageMenu />
        </div>
      </header>

      <main className="grid min-h-0 flex-1 grid-cols-[minmax(360px,420px)_1fr] overflow-hidden">
        <section className="min-h-0 overflow-y-auto border-r border-border p-5">
          <div className="mb-5">
            <div className="flex items-center gap-2 text-sm font-semibold">
              <Settings2 className="h-4 w-4 text-primary" />
              {t("Setup targets")}
            </div>
            <p className="mt-1 text-xs text-muted-foreground">
              {t("Pick what you want to use. Startkit only installs what these choices require.")}
            </p>
          </div>

          <div className="space-y-6">
            <SourceChooser
              sources={manifest?.sources ?? {}}
              value={downloadSource}
              onChange={setDownloadSource}
            />
            <ToolchainChooser value={toolchainMode} onChange={setToolchainMode} />

            <div className="rounded-md border border-border bg-card/40 p-3">
              <StepAgents
                agents={agents}
                profiles={profiles}
                enabled={enabledAgents}
                onToggle={toggleAgent}
                onCreateProfile={() => setProfileEditorOpen(true)}
                onDeleteProfile={(id) => {
                  void handleDeleteProfile(id);
                }}
              />
            </div>

            <div className="rounded-md border border-border bg-card/40 p-3">
              <StepTunnel
                tunnels={tunnels}
                provider={tunnelProvider}
                onProvider={setTunnelProvider}
                ngrokToken={ngrokToken}
                onNgrokToken={setNgrokToken}
                ngrokDomain={ngrokDomain}
                onNgrokDomain={setNgrokDomain}
                cfToken={cfToken}
                onCfToken={setCfToken}
                cfHostname={cfHostname}
                onCfHostname={setCfHostname}
              />
            </div>

            <div className="rounded-md border border-border bg-card/40 p-3">
              <StepChannels
                pluginRegistry={pluginRegistry}
                discoveredPlugins={discoveredPlugins}
                enabledChannels={enabledChannels}
                channelConfigs={channelConfigs}
                channelVerbose={channelVerbose}
                installingPlugins={installingPlugins}
                authStates={authStates}
                onToggleChannel={toggleChannel}
                onConfigChange={updateChannelConfig}
                onVerboseChange={updateChannelVerbose}
                onInstallPlugin={installPlugin}
                onStartAuth={startAuth}
                onCancelAuth={cancelAuth}
                switchSize="sm"
              />
            </div>
          </div>
        </section>

        <section className="flex min-h-0 flex-col">
          <div className="border-b border-border p-5">
            <div className="flex items-start justify-between gap-4">
              <div>
                <div className="flex items-center gap-2 text-sm font-semibold">
                  <Wrench className="h-4 w-4 text-primary" />
                  {t("Computer readiness")}
                </div>
                <p className="mt-1 max-w-2xl text-xs text-muted-foreground">
                  {t("Scan first, then let Startkit install or repair the pieces required by your selected toolchain mode.")}
                </p>
              </div>
              <div className="flex shrink-0 items-center gap-2">
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => void startkit.scan(finalSettings, choices)}
                  disabled={startkit.scanning || startkit.running}
                >
                  {startkit.scanning ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <RefreshCw className="h-4 w-4" />
                  )}
                  {t("Scan")}
                </Button>
                {startkit.running ? (
                  <Button
                    type="button"
                    variant="outline"
                    onClick={() => void startkit.cancel()}
                  >
                    {t("Cancel")}
                  </Button>
                ) : (
                  <Button
                    type="button"
                    onClick={() => void startkit.start(finalSettings, choices)}
                    disabled={startkit.scanning}
                  >
                    <Download className="h-4 w-4" />
                    {t("Fix selected")}
                  </Button>
                )}
                <Button
                  type="button"
                  onClick={() => void startkit.finish()}
                  disabled={!canFinish || startkit.running}
                >
                  <Rocket className="h-4 w-4" />
                  {startkit.finalStatus === "error"
                    ? t("Continue Anyway")
                    : t("Open VibeAround")}
                </Button>
              </div>
            </div>
            {startkit.error && (
              <div className="mt-3 rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive">
                {startkit.error}
              </div>
            )}
          </div>

          <div className="min-h-0 flex-1 overflow-y-auto p-5">
            {groupedReports.length === 0 ? (
              <EmptyPlan />
            ) : (
              <div className="space-y-4">
                {groupedReports.map((group) => (
                  <section
                    key={group.id}
                    className="rounded-md border border-border bg-card/40"
                  >
                    <div className="flex items-center justify-between gap-3 border-b border-border px-4 py-3">
                      <div className="flex items-center gap-2">
                        {groupIcon(group.id)}
                        <div>
                          <div className="text-sm font-medium">
                            {groupTitle(group.id)}
                          </div>
                          <div className="text-[11px] text-muted-foreground">
                            {groupSummary(group.reports)}
                          </div>
                        </div>
                      </div>
                    </div>
                    <div className="divide-y divide-border">
                      {group.reports.map((report) => (
                        <StartkitReportRow key={report.id} report={report} />
                      ))}
                    </div>
                  </section>
                ))}
              </div>
            )}
          </div>
        </section>
      </main>

      {profileEditorOpen && (
        <ProfileFormDialog
          catalog={catalog}
          initial={null}
          onClose={() => setProfileEditorOpen(false)}
          onSave={handleSaveProfile}
        />
      )}
    </div>
  );
}

function ToolchainChooser({
  value,
  onChange,
}: {
  value: "auto" | "managed" | "system";
  onChange: (value: "auto" | "managed" | "system") => void;
}) {
  const options: Array<{
    id: "auto" | "managed" | "system";
    label: string;
    description: string;
  }> = [
    {
      id: "auto",
      label: "Auto",
      description: "Reuse valid system tools; install managed copies when needed.",
    },
    {
      id: "managed",
      label: "Managed",
      description: "Install and prefer VibeAround-managed Node and CLIs.",
    },
    {
      id: "system",
      label: "System only",
      description: "Do not install managed tools; use what is already on PATH.",
    },
  ];

  return (
    <div className="space-y-2">
      <div className="flex items-center gap-2 text-xs font-medium">
        <TerminalSquare className="h-3.5 w-3.5 text-primary" />
        Toolchain mode
      </div>
      <div className="grid gap-2">
        {options.map((option) => (
          <button
            key={option.id}
            type="button"
            className={cn(
              "rounded-md border p-2 text-left transition-colors",
              value === option.id
                ? "border-primary bg-primary/10 text-foreground"
                : "border-border bg-background hover:border-primary/30",
            )}
            onClick={() => onChange(option.id)}
          >
            <span className="block text-xs font-medium">{option.label}</span>
            <span className="mt-0.5 block text-[11px] leading-snug text-muted-foreground">
              {option.description}
            </span>
          </button>
        ))}
      </div>
    </div>
  );
}

function SourceChooser({
  sources,
  value,
  onChange,
}: {
  sources: StartkitManifestSummary["sources"];
  value: string;
  onChange: (value: string) => void;
}) {
  const entries: Array<[string, { label: string }]> =
    Object.keys(sources).length > 0
      ? Object.entries(sources)
      : [
          ["global", { label: "Global" }],
          ["cn", { label: "China mirror" }],
        ];
  return (
    <div className="space-y-2">
      <div className="flex items-center gap-2 text-xs font-medium">
        <Globe className="h-3.5 w-3.5 text-primary" />
        Download source
      </div>
      <div className="grid grid-cols-2 gap-2">
        {entries.map(([id, source]) => (
          <Button
            key={id}
            type="button"
            size="sm"
            variant="outline"
            className={cn(
              "justify-center text-xs",
              value === id && "border-primary bg-primary/10 text-primary",
            )}
            onClick={() => onChange(id)}
          >
            {source.label}
          </Button>
        ))}
      </div>
    </div>
  );
}

function EmptyPlan() {
  return (
    <div className="flex h-full min-h-[320px] items-center justify-center rounded-md border border-dashed border-border">
      <div className="max-w-sm text-center">
        <Circle className="mx-auto mb-3 h-8 w-8 text-muted-foreground/50" />
        <div className="text-sm font-medium">Ready to scan</div>
        <p className="mt-1 text-xs text-muted-foreground">
          Choose agents, tunnel, and messaging targets on the left, then scan
          this computer.
        </p>
      </div>
    </div>
  );
}

function StartkitReportRow({ report }: { report: StartkitItemReport }) {
  return (
    <div className="grid grid-cols-[minmax(180px,1fr)_120px_minmax(180px,1.3fr)] items-center gap-3 px-4 py-3">
      <div className="min-w-0">
        <div className="truncate text-sm font-medium">{report.label}</div>
        {report.path && (
          <div className="mt-0.5 truncate font-mono text-[10px] text-muted-foreground">
            {report.path}
          </div>
        )}
      </div>
      <div className={cn("inline-flex w-fit items-center gap-1.5 rounded border px-2 py-1 text-[11px]", statusClass(report.status))}>
        {statusIcon(report.status)}
        {statusLabel(report.status)}
      </div>
      <div className="min-w-0 text-xs text-muted-foreground">
        <div className="truncate">
          {report.message ?? report.version ?? "Waiting for scan"}
        </div>
        {report.version && report.message && (
          <div className="mt-0.5 truncate font-mono text-[10px] opacity-80">
            {report.version}
          </div>
        )}
      </div>
    </div>
  );
}

function groupReports(
  planItems: { id: string; group: string; label: string; category: string; severity?: string; secret: boolean; settingsKey?: string }[],
  reportById: Map<string, StartkitItemReport>,
) {
  const groups = new Map<string, StartkitItemReport[]>();
  for (const item of planItems) {
    const report =
      reportById.get(item.id) ??
      ({
        id: item.id,
        label: item.label,
        group: item.group,
        category: item.category,
        status: "pending",
        severity: item.severity,
        actions: [],
        secret: item.secret,
        settingsKey: item.settingsKey,
      } satisfies StartkitItemReport);
    if (!groups.has(report.group)) groups.set(report.group, []);
    groups.get(report.group)!.push(report);
  }
  return Array.from(groups.entries())
    .sort(
      ([a], [b]) =>
        (GROUP_ORDER.indexOf(a) < 0 ? 99 : GROUP_ORDER.indexOf(a)) -
        (GROUP_ORDER.indexOf(b) < 0 ? 99 : GROUP_ORDER.indexOf(b)),
    )
    .map(([id, reports]) => ({ id, reports }));
}

function groupTitle(id: string): string {
  switch (id) {
    case "computer":
      return "Computer basics";
    case "agents":
      return "Coding agents";
    case "remote":
      return "Remote access";
    case "messaging":
      return "Messaging";
    default:
      return id;
  }
}

function groupIcon(id: string) {
  const className = "h-4 w-4 text-primary";
  switch (id) {
    case "agents":
      return <TerminalSquare className={className} />;
    case "remote":
      return <Globe className={className} />;
    case "messaging":
      return <MessageSquare className={className} />;
    default:
      return <Settings2 className={className} />;
  }
}

function groupSummary(reports: StartkitItemReport[]): string {
  const counts = reports.reduce<Record<string, number>>((acc, report) => {
    acc[report.status] = (acc[report.status] ?? 0) + 1;
    return acc;
  }, {});
  if (counts.error || counts.blocked) return "Needs attention";
  if (counts.needs_config) return "Needs configuration";
  if (counts.missing || counts.outdated || counts.broken) return "Fixes available";
  if (counts.running) return "Running";
  if (counts.ok && counts.ok === reports.length) return "Ready";
  return `${reports.length} item${reports.length === 1 ? "" : "s"}`;
}

function statusClass(status: StartkitStatus): string {
  switch (status) {
    case "ok":
      return "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
    case "running":
      return "border-primary/30 bg-primary/10 text-primary";
    case "missing":
    case "outdated":
    case "broken":
    case "needs_config":
      return "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300";
    case "blocked":
    case "error":
      return "border-destructive/30 bg-destructive/10 text-destructive";
    case "skipped":
      return "border-border bg-muted text-muted-foreground";
    default:
      return "border-border bg-background text-muted-foreground";
  }
}

function statusIcon(status: StartkitStatus) {
  const className = "h-3.5 w-3.5";
  switch (status) {
    case "ok":
      return <CheckCircle2 className={className} />;
    case "running":
      return <Loader2 className={`${className} animate-spin`} />;
    case "blocked":
    case "error":
      return <AlertCircle className={className} />;
    default:
      return <Circle className={className} />;
  }
}

function statusLabel(status: StartkitStatus): string {
  switch (status) {
    case "needs_config":
      return "needs config";
    default:
      return status.replace("_", " ");
  }
}

function defaultChannelVerbose(): ChannelVerboseConfig {
  return {
    show_thinking: false,
    show_tool_use: false,
  };
}

function parseChannelVerbose(value: unknown): ChannelVerboseConfig {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return defaultChannelVerbose();
  }
  const verbose = value as Record<string, unknown>;
  return {
    show_thinking:
      typeof verbose.show_thinking === "boolean"
        ? verbose.show_thinking
        : false,
    show_tool_use:
      typeof verbose.show_tool_use === "boolean"
        ? verbose.show_tool_use
        : false,
  };
}
