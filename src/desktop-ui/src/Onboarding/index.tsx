import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ArrowRight,
  Download,
  Loader2,
  Rocket,
} from "lucide-react";
import { useI18n } from "@va/i18n";

import { LanguageMenu } from "@/components/LanguageMenu";
import { cn } from "@/lib/utils";

import {
  OnboardingFooter,
  type PrimaryAction,
} from "./components/OnboardingFooter";
import { OnboardingStepContent } from "./components/OnboardingStepContent";
import { StartkitAdvancedMenu } from "./components/StartkitAdvancedMenu";
import { ProgressStepper, QuestionPane } from "./components/WizardChrome";
import { groupReports, reportNeedsInstall } from "./components/startkitPresentation";
import { useChannelAuth } from "./hooks/useChannelAuth";
import { useStartkitFlow } from "./hooks/useStartkitFlow";
import { defaultChannelVerbose } from "./lib/channelConfig";
import { buildSettings } from "./lib/buildSettings";
import { useOnboardingInitialLoad } from "./hooks/useOnboardingInitialLoad";
import { WIZARD_STEPS, type WizardStepId } from "./wizardTypes";
import {
  createProfile,
  deleteProfile,
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
  StartkitManifestSummary,
  TunnelSummary,
} from "./types";
import type { AgentId, TunnelProvider } from "./constants";

export default function Onboarding() {
  const { t } = useI18n();
  const isMacTitlebar =
    typeof navigator !== "undefined" && /Mac/.test(navigator.platform);

  const [settings, setSettings] = useState<Settings>({});
  const [loaded, setLoaded] = useState(false);
  const [activeStep, setActiveStep] = useState<WizardStepId>("agents");
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
  const [shellPath, setShellPath] = useState(false);
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
  const [finishError, setFinishError] = useState<string | null>(null);
  const [finishing, setFinishing] = useState(false);

  const startkit = useStartkitFlow();
  const autoScanSignatureRef = useRef<string | null>(null);
  const refreshedPluginsAfterInstallRef = useRef(false);

  useOnboardingInitialLoad({
    setSettings,
    setLoaded,
    setManifest,
    setCatalog,
    setProfiles,
    setAgents,
    setTunnels,
    setPluginRegistry,
    setDiscoveredPlugins,
    setDownloadSource,
    setToolchainMode,
    setShellPath,
    setEnabledAgents,
    setEnabledChannels,
    setChannelConfigs,
    setChannelVerbose,
    setTunnelProvider,
    setNgrokToken,
    setNgrokDomain,
    setCfToken,
    setCfHostname,
  });

  useEffect(() => {
    if (toolchainMode !== "auto") setToolchainMode("auto");
  }, [toolchainMode]);

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
      shellPath: toolchainMode === "system" ? false : shellPath,
    }),
    [
      enabledAgents,
      tunnelProvider,
      enabledChannels,
      downloadSource,
      toolchainMode,
      shellPath,
    ],
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
          shell_path: toolchainMode === "system" ? false : shellPath,
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
      shellPath,
    ],
  );

  const scanSignature = useMemo(() => JSON.stringify(choices), [choices]);

  useEffect(() => {
    if (!loaded || startkit.running) return;
    if (autoScanSignatureRef.current === scanSignature) return;
    autoScanSignatureRef.current = scanSignature;

    const timer = window.setTimeout(() => {
      void startkit.scan(finalSettings, choices);
    }, 250);
    return () => window.clearTimeout(timer);
  }, [loaded, scanSignature, startkit.running, startkit.scan, finalSettings, choices]);

  useEffect(() => {
    if (startkit.running) {
      refreshedPluginsAfterInstallRef.current = false;
      return;
    }
    if (!startkit.complete || refreshedPluginsAfterInstallRef.current) return;
    refreshedPluginsAfterInstallRef.current = true;

    void invoke<DiscoveredChannelPlugin[]>("list_channel_plugins")
      .then(setDiscoveredPlugins)
      .catch((error) => {
        console.error("failed to refresh channel plugins", error);
      });
  }, [startkit.complete, startkit.running]);

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
    active: activeStep === "configure",
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

  const finishOnboarding = useCallback(async () => {
    setFinishing(true);
    setFinishError(null);
    try {
      await invoke("save_settings", { settings: finalSettings });
      await startkit.finish();
    } catch (error) {
      setFinishError(String(error));
      setFinishing(false);
    }
  }, [finalSettings, startkit.finish]);

  const groupedReports = useMemo(
    () => groupReports(startkit.plan?.items ?? [], startkit.reportById),
    [startkit.plan, startkit.reportById],
  );
  const hasScanned = startkit.reports.some((report) => report.status !== "pending");
  const hasInstallWork = startkit.reports.some(reportNeedsInstall);
  const hasBlockingReport = startkit.reports.some((report) =>
    ["blocked", "error"].includes(report.status),
  );
  const canContinueFromInstall =
    startkit.complete || (hasScanned && !hasInstallWork && !hasBlockingReport);
  const activeIndex = WIZARD_STEPS.findIndex((step) => step.id === activeStep);

  const goNext = useCallback(() => {
    if (activeStep === "agents") setActiveStep("im");
    else if (activeStep === "im") setActiveStep("remote");
    else if (activeStep === "remote") setActiveStep("install");
    else if (activeStep === "install") setActiveStep("configure");
  }, [activeStep]);

  const goBack = useCallback(() => {
    if (activeStep === "im") setActiveStep("agents");
    else if (activeStep === "remote") setActiveStep("im");
    else if (activeStep === "install") setActiveStep("remote");
    else if (activeStep === "configure") setActiveStep("install");
  }, [activeStep]);

  const skipStep = useCallback(() => {
    if (activeStep === "agents") {
      setEnabledAgents(new Set());
      setActiveStep("im");
    } else if (activeStep === "im") {
      setEnabledChannels(new Set());
      setActiveStep("remote");
    } else if (activeStep === "remote") {
      setTunnelProvider("none");
      setActiveStep("install");
    }
  }, [activeStep]);

  const primaryAction = useMemo<PrimaryAction>(() => {
    if (activeStep === "install") {
      if (startkit.running) {
        return {
          label: t("Installing..."),
          icon: <Loader2 className="h-4 w-4 animate-spin" />,
          disabled: true,
          run: () => {},
        };
      }
      if (startkit.scanning && !hasScanned) {
        return {
          label: t("Checking..."),
          icon: <Loader2 className="h-4 w-4 animate-spin" />,
          disabled: true,
          run: () => {},
        };
      }
      if (canContinueFromInstall) {
        return {
          label: t("Continue to configuration"),
          icon: <ArrowRight className="h-4 w-4" />,
          disabled: false,
          run: () => setActiveStep("configure"),
        };
      }
      return {
        label: t("Install selected"),
        icon: <Download className="h-4 w-4" />,
        disabled: startkit.scanning,
        run: () => void startkit.start(finalSettings, choices),
      };
    }

    if (activeStep === "configure") {
      return {
        label: finishing ? t("Launching...") : t("Launch VibeAround"),
        icon: finishing ? (
          <Loader2 className="h-4 w-4 animate-spin" />
        ) : (
          <Rocket className="h-4 w-4" />
        ),
        disabled: finishing,
        run: () => void finishOnboarding(),
      };
    }

    return {
      label: t("Continue"),
      icon: <ArrowRight className="h-4 w-4" />,
      disabled: false,
      run: goNext,
    };
  }, [
    activeStep,
    canContinueFromInstall,
    choices,
    finalSettings,
    finishOnboarding,
    finishing,
    goNext,
    hasScanned,
    startkit,
    t,
  ]);

  if (!loaded) {
    return (
      <div className="flex h-full items-center justify-center">
        <span className="text-sm text-muted-foreground animate-pulse">
          {t("Loading...")}
        </span>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col bg-background">
      <header
        className={cn(
          "relative flex h-12 items-center gap-4 border-b border-border pr-3",
          isMacTitlebar ? "pl-[82px]" : "pl-3",
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
        <div className="absolute left-1/2 top-1/2 z-10 -translate-x-1/2 -translate-y-1/2">
          <ProgressStepper activeIndex={activeIndex} />
        </div>
        <div className="relative z-10 ml-auto flex shrink-0 items-center gap-1">
          <StartkitAdvancedMenu
            sources={manifest?.sources ?? {}}
            downloadSource={downloadSource}
            onDownloadSource={setDownloadSource}
            shellPath={shellPath && toolchainMode !== "system"}
            shellPathDisabled={toolchainMode === "system"}
            onShellPath={setShellPath}
          />
          <LanguageMenu />
        </div>
      </header>

      <main className="grid min-h-0 flex-1 grid-cols-[minmax(320px,430px)_1fr] overflow-hidden">
        <QuestionPane
          step={activeStep}
        />

        <OnboardingStepContent
          activeStep={activeStep}
          agents={agents}
          enabledAgents={enabledAgents}
          reportsById={startkit.reportById}
          scanning={startkit.scanning}
          onToggleAgent={toggleAgent}
          pluginRegistry={pluginRegistry}
          discoveredPlugins={discoveredPlugins}
          enabledChannels={enabledChannels}
          onToggleChannel={toggleChannel}
          tunnels={tunnels}
          tunnelProvider={tunnelProvider}
          onTunnelProvider={setTunnelProvider}
          groupedReports={groupedReports}
          reports={startkit.reports}
          running={startkit.running}
          complete={startkit.complete}
          finalStatus={startkit.finalStatus}
          startkitError={startkit.error}
          choices={choices}
          profiles={profiles}
          channelConfigs={channelConfigs}
          channelVerbose={channelVerbose}
          installingPlugins={installingPlugins}
          authStates={authStates}
          ngrokToken={ngrokToken}
          ngrokDomain={ngrokDomain}
          cfToken={cfToken}
          cfHostname={cfHostname}
          finishError={finishError}
          onCreateProfile={() => setProfileEditorOpen(true)}
          onDeleteProfile={(id) => {
            void handleDeleteProfile(id);
          }}
          onConfigChange={updateChannelConfig}
          onVerboseChange={updateChannelVerbose}
          onInstallPlugin={installPlugin}
          onStartAuth={startAuth}
          onCancelAuth={cancelAuth}
          onNgrokToken={setNgrokToken}
          onNgrokDomain={setNgrokDomain}
          onCfToken={setCfToken}
          onCfHostname={setCfHostname}
        />
      </main>

      <OnboardingFooter
        activeStep={activeStep}
        activeIndex={activeIndex}
        running={startkit.running}
        finishing={finishing}
        primaryAction={primaryAction}
        onBack={goBack}
        onSkip={skipStep}
        onCancel={() => void startkit.cancel()}
      />

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
