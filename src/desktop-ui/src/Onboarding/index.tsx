import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ArrowRight,
  Download,
  Loader2,
  RefreshCw,
  Rocket,
} from "lucide-react";
import { useI18n } from "@va/i18n";

import { LanguageMenu } from "@/components/LanguageMenu";
import { cn } from "@/lib/utils";

import {
  OnboardingFooter,
  type FooterAction,
  type PrimaryAction,
} from "./components/OnboardingFooter";
import { OnboardingStepContent } from "./components/OnboardingStepContent";
import { StartkitAdvancedMenu } from "./components/StartkitAdvancedMenu";
import { ProgressStepper, QuestionPane } from "./components/WizardChrome";
import { useChannelAuth } from "./hooks/useChannelAuth";
import { useStartkitFlow } from "./hooks/useStartkitFlow";
import { defaultChannelVerbose } from "./lib/channelConfig";
import { buildSettings } from "./lib/buildSettings";
import { useOnboardingInitialLoad } from "./hooks/useOnboardingInitialLoad";
import {
  createSettingsPatch,
  saveSettingsPatch,
} from "../lib/settingsPatch";
import {
  agentCheckingReport,
  agentIdFromReport,
  groupReportsFromReports,
  itemCheckSignature,
  localPluginReport,
  markReportsUpdating,
  mergeLocalReportsById,
  mergeReportsById,
  pluginCheckingReport,
  pluginIdFromReport,
  tunnelCheckingReport,
  tunnelReportMatchesProvider,
} from "./lib/checkReports";
import { WIZARD_STEPS, type WizardStepId } from "./wizardTypes";
import type {
  AgentSummary,
  ChannelVerboseConfig,
  DiscoveredChannelPlugin,
  PluginRegistryEntry,
  Settings,
  StartkitChoices,
  StartkitItemReport,
  StartkitManifestSummary,
  ToolchainMode,
  TunnelSummary,
} from "./types";
import type { AgentId, TunnelProvider } from "./constants";

const DEFAULT_TOOLCHAIN_MODE: ToolchainMode = "system";

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
  const [agents, setAgents] = useState<AgentSummary[]>([]);
  const [tunnels, setTunnels] = useState<TunnelSummary[]>([]);
  const [pluginRegistry, setPluginRegistry] = useState<PluginRegistryEntry[]>(
    [],
  );
  const [discoveredPlugins, setDiscoveredPlugins] = useState<
    DiscoveredChannelPlugin[]
  >([]);

  const [downloadSource, setDownloadSource] = useState("global");
  const [toolchainMode, setToolchainMode] =
    useState<ToolchainMode>(DEFAULT_TOOLCHAIN_MODE);
  const [portableToolchain, setPortableToolchain] = useState(false);
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
    useState<TunnelProvider>("none");
  const [ngrokToken, setNgrokToken] = useState("");
  const [ngrokDomain, setNgrokDomain] = useState("");
  const [cfToken, setCfToken] = useState("");
  const [cfHostname, setCfHostname] = useState("");
  const [finishError, setFinishError] = useState<string | null>(null);
  const [finishing, setFinishing] = useState(false);

  const startkit = useStartkitFlow();
  const checkedAgentLocalSignaturesRef = useRef<Set<string>>(new Set());
  const checkedAgentUpdateSignaturesRef = useRef<Set<string>>(new Set());
  const checkedPluginSignaturesRef = useRef<Set<string>>(new Set());
  const checkedTunnelSignaturesRef = useRef<Set<string>>(new Set());
  const checkedInstallScanSignaturesRef = useRef<Set<string>>(new Set());
  const previousStartkitOptionsRef = useRef<string | null>(null);
  const refreshedPluginsAfterInstallRef = useRef(false);
  const [agentInstallReports, setAgentInstallReports] = useState<
    StartkitItemReport[]
  >([]);
  const [pluginUpdateReports, setPluginUpdateReports] = useState<
    StartkitItemReport[]
  >([]);
  const [tunnelReports, setTunnelReports] = useState<StartkitItemReport[]>([]);

  useOnboardingInitialLoad({
    setSettings,
    setLoaded,
    setManifest,
    setAgents,
    setTunnels,
    setPluginRegistry,
    setDiscoveredPlugins,
    setDownloadSource,
    setToolchainMode,
    setPortableToolchain,
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
      portableToolchain,
      shellPath: false,
    }),
    [
      enabledAgents,
      tunnelProvider,
      enabledChannels,
      downloadSource,
      toolchainMode,
      portableToolchain,
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
          portable_toolchain: portableToolchain,
          shell_path: false,
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
      portableToolchain,
    ],
  );
  const settingsPatch = useMemo(
    () => createSettingsPatch(settings, finalSettings),
    [finalSettings, settings],
  );

  const agentStatusChoices = useMemo<StartkitChoices>(
    () => ({
      agents: [],
      tunnel: "none",
      channels: [],
      source: downloadSource,
      toolchainMode,
      portableToolchain,
      shellPath: false,
    }),
    [downloadSource, toolchainMode, portableToolchain],
  );

  const checkAgentUpdates = useCallback(
    (agentIds: string[]) => {
      const pendingAgentIds = agentIds.filter((agentId) => {
        const signature = itemCheckSignature(
          agentId,
          downloadSource,
          toolchainMode,
          portableToolchain ? "portable" : "system-runtime",
        );
        return !checkedAgentUpdateSignaturesRef.current.has(signature);
      });
      if (pendingAgentIds.length === 0) return;
      for (const agentId of pendingAgentIds) {
        checkedAgentUpdateSignaturesRef.current.add(
          itemCheckSignature(
            agentId,
            downloadSource,
            toolchainMode,
            portableToolchain ? "portable" : "system-runtime",
          ),
        );
      }

      const reportIds = new Set(
        pendingAgentIds.map((agentId) => `agents.${agentId}.cli`),
      );
      setAgentInstallReports((previous) =>
        markReportsUpdating(previous, reportIds, "Checking updates"),
      );

      void invoke<StartkitItemReport[]>("check_agent_updates", {
        request: {
          agentIds: pendingAgentIds,
          choices: {
            ...agentStatusChoices,
            agents: pendingAgentIds,
          },
        },
      })
        .then((reports) => {
          setAgentInstallReports((previous) =>
            mergeReportsById(previous, reports),
          );
        })
        .catch((error) => {
          console.error("failed to check agent updates", error);
          setAgentInstallReports((previous) =>
            markReportsUpdating(previous, reportIds, "Unable to check updates"),
          );
        });
    },
    [agentStatusChoices, downloadSource, toolchainMode, portableToolchain],
  );

  useEffect(() => {
    if (!loaded) return;
    const signature = itemCheckSignature(
      "startkit-options",
      downloadSource,
      toolchainMode,
      portableToolchain ? "portable" : "system-runtime",
    );
    if (previousStartkitOptionsRef.current === null) {
      previousStartkitOptionsRef.current = signature;
      return;
    }
    if (previousStartkitOptionsRef.current === signature) return;

    previousStartkitOptionsRef.current = signature;
    checkedAgentLocalSignaturesRef.current.clear();
    checkedAgentUpdateSignaturesRef.current.clear();
    checkedPluginSignaturesRef.current.clear();
    checkedTunnelSignaturesRef.current.clear();
    checkedInstallScanSignaturesRef.current.clear();
    setAgentInstallReports([]);
    setPluginUpdateReports([]);
    setTunnelReports([]);
    startkit.reset();
  }, [downloadSource, loaded, portableToolchain, startkit.reset, toolchainMode]);

  useEffect(() => {
    if (!loaded || activeStep !== "install" || startkit.running || startkit.scanning) return;
    const signature = itemCheckSignature(
      "install",
      downloadSource,
      [...choices.agents].sort().join(","),
      [...choices.channels].sort().join(","),
      choices.tunnel,
      choices.toolchainMode,
      choices.portableToolchain ? "portable" : "system-runtime",
    );
    if (checkedInstallScanSignaturesRef.current.has(signature)) return;
    checkedInstallScanSignaturesRef.current.add(signature);

    void startkit.scan(finalSettings, choices);
  }, [
    activeStep,
    choices,
    downloadSource,
    finalSettings,
    loaded,
    startkit.running,
    startkit.scanning,
    startkit.scan,
  ]);

  useEffect(() => {
    if (!loaded || activeStep !== "agents" || agents.length === 0 || startkit.running) return;
    const agentIds = agents.map((agent) => agent.id).sort();
    const pendingAgentIds = agentIds.filter((agentId) => {
      const signature = itemCheckSignature(
        agentId,
        "local",
        toolchainMode,
        portableToolchain ? "portable" : "system-runtime",
      );
      return !checkedAgentLocalSignaturesRef.current.has(signature);
    });
    if (pendingAgentIds.length === 0) return;
    for (const agentId of pendingAgentIds) {
      checkedAgentLocalSignaturesRef.current.add(
        itemCheckSignature(
          agentId,
          "local",
          toolchainMode,
          portableToolchain ? "portable" : "system-runtime",
        ),
      );
    }
    setAgentInstallReports((previous) =>
      mergeReportsById(
        previous,
        pendingAgentIds.map((agentId) =>
          agentCheckingReport(agentId, agents, "Checking local version"),
        ),
      ),
    );

    void invoke<StartkitItemReport[]>("scan_agent_install_status", {
      settings,
      choices: {
        ...agentStatusChoices,
        agents: pendingAgentIds,
      },
    })
      .then((reports) => {
        setAgentInstallReports((previous) =>
          mergeReportsById(previous, reports),
        );
      })
      .catch((error) => {
        console.error("failed to scan agent install status", error);
      });
  }, [
    activeStep,
    agentStatusChoices,
    agents,
    loaded,
    settings,
    startkit.running,
    toolchainMode,
    portableToolchain,
  ]);

  useEffect(() => {
    if (!loaded || activeStep !== "agents" || startkit.running) return;
    const updateAgentIds = agentInstallReports
      .filter((report) => report.status === "ok")
      .map(agentIdFromReport)
      .filter((id): id is string => Boolean(id));
    if (updateAgentIds.length > 0) {
      checkAgentUpdates(updateAgentIds);
    }
  }, [
    activeStep,
    agentInstallReports,
    checkAgentUpdates,
    loaded,
    startkit.running,
  ]);

  useEffect(() => {
    if (!loaded || (activeStep !== "im" && activeStep !== "install")) return;
    if (pluginRegistry.length === 0) return;
    setPluginUpdateReports((previous) =>
      mergeLocalReportsById(
        previous,
        pluginRegistry.map((entry) =>
          localPluginReport(entry, discoveredPlugins),
        ),
      ),
    );
  }, [activeStep, discoveredPlugins, loaded, pluginRegistry]);

  useEffect(() => {
    if (!loaded || (activeStep !== "im" && activeStep !== "install") || startkit.running) return;
    const pluginIds = Array.from(enabledChannels).sort();
    if (pluginIds.length === 0) return;
    const pendingPluginIds = pluginIds.filter((id) => {
      const discovered = discoveredPlugins.find((plugin) => plugin.id === id);
      const signature = itemCheckSignature(
        id,
        discovered?.version ?? "not-installed",
        "plugin",
      );
      return !checkedPluginSignaturesRef.current.has(signature);
    });
    if (pendingPluginIds.length === 0) return;
    for (const id of pendingPluginIds) {
      const discovered = discoveredPlugins.find((plugin) => plugin.id === id);
      checkedPluginSignaturesRef.current.add(
        itemCheckSignature(id, discovered?.version ?? "not-installed", "plugin"),
      );
    }

    for (const pluginId of pendingPluginIds) {
      setPluginUpdateReports((previous) =>
        mergeReportsById(previous, [
          pluginCheckingReport(pluginId, pluginRegistry, discoveredPlugins),
        ]),
      );

      void invoke<StartkitItemReport[]>("check_plugin_updates", {
        request: { pluginIds: [pluginId] },
      })
        .then((reports) => {
          setPluginUpdateReports((previous) =>
            mergeReportsById(previous, reports),
          );
        })
        .catch((error) => {
          console.error(`failed to check ${pluginId} plugin updates`, error);
        });
    }
  }, [
    activeStep,
    discoveredPlugins,
    enabledChannels,
    loaded,
    pluginRegistry,
    startkit.running,
  ]);

  useEffect(() => {
    if (!loaded || activeStep !== "remote" || tunnels.length === 0) return;
    const tunnelIds = tunnels
      .map((tunnel) => tunnel.id)
      .filter((id) => id !== "none")
      .sort();
    const pendingTunnelIds = tunnelIds.filter((id) => {
      const signature = itemCheckSignature(
        id,
        "tunnel",
        toolchainMode,
        portableToolchain ? "portable" : "system-runtime",
      );
      return !checkedTunnelSignaturesRef.current.has(signature);
    });
    if (pendingTunnelIds.length === 0) return;
    for (const id of pendingTunnelIds) {
      checkedTunnelSignaturesRef.current.add(
        itemCheckSignature(
          id,
          "tunnel",
          toolchainMode,
          portableToolchain ? "portable" : "system-runtime",
        ),
      );
    }

    for (const tunnelId of pendingTunnelIds) {
      setTunnelReports((previous) =>
        mergeReportsById(previous, [
          tunnelCheckingReport(tunnelId, tunnels),
        ]),
      );

      void invoke<StartkitItemReport[]>("scan_tunnel_status", {
        settings,
        choices: {
          ...agentStatusChoices,
          tunnel: tunnelId,
        },
      })
        .then((reports) => {
          setTunnelReports((previous) =>
            mergeReportsById(previous, reports),
          );
        })
        .catch((error) => {
          console.error(`failed to scan ${tunnelId} tunnel status`, error);
        });
    }
  }, [
    activeStep,
    agentStatusChoices,
    loaded,
    settings,
    tunnels,
    toolchainMode,
    portableToolchain,
  ]);

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

  const finishOnboarding = useCallback(async () => {
    setFinishing(true);
    setFinishError(null);
    try {
      await saveSettingsPatch(settings, finalSettings);
      await startkit.finish();
    } catch (error) {
      setFinishError(String(error));
      setFinishing(false);
    }
  }, [finalSettings, settings, startkit.finish]);

  const cachedInstallReports = useMemo(() => {
    const selectedAgents = new Set(choices.agents);
    const selectedChannels = new Set(choices.channels);
    return mergeReportsById([], [
      ...agentInstallReports.filter((report) => {
        const agentId = agentIdFromReport(report);
        return agentId ? selectedAgents.has(agentId) : false;
      }),
      ...pluginUpdateReports.filter((report) => {
        const pluginId = pluginIdFromReport(report);
        return pluginId ? selectedChannels.has(pluginId) : false;
      }),
      ...tunnelReports.filter((report) =>
        tunnelReportMatchesProvider(report, choices.tunnel),
      ),
    ]);
  }, [
    agentInstallReports,
    choices.agents,
    choices.channels,
    choices.tunnel,
    pluginUpdateReports,
    tunnelReports,
  ]);
  const installReports = useMemo(() => {
    if (startkit.running || startkit.complete) return startkit.reports;
    if (startkit.scanning) {
      return mergeReportsById(cachedInstallReports, startkit.reports);
    }
    if (startkit.reports.length > 0) {
      return mergeReportsById(cachedInstallReports, startkit.reports);
    }
    return cachedInstallReports;
  }, [
    cachedInstallReports,
    startkit.complete,
    startkit.reports,
    startkit.running,
    startkit.scanning,
  ]);
  const groupedReports = useMemo(
    () => groupReportsFromReports(installReports),
    [installReports],
  );
  const agentReportsById = useMemo(() => {
    const reports = new Map(agentInstallReports.map((report) => [report.id, report]));
    for (const [id, report] of startkit.reportById.entries()) {
      if (id.startsWith("agents.")) reports.set(id, report);
    }
    return reports;
  }, [agentInstallReports, startkit.reportById]);

  useEffect(() => {
    if (!loaded || activeStep !== "install" || startkit.running) return;
    const updateAgentIds = installReports
      .filter((report) => report.status === "ok")
      .map(agentIdFromReport)
      .filter((id): id is string => Boolean(id));
    if (updateAgentIds.length > 0) {
      checkAgentUpdates(updateAgentIds);
    }
  }, [
    activeStep,
    checkAgentUpdates,
    installReports,
    loaded,
    startkit.running,
  ]);
  const hasScanned = installReports.some((report) => report.status !== "pending");
  const installReportsRunning = installReports.some((report) => report.status === "running");
  const hasRunnableInstallWork = installReports.some((report) =>
    report.actions.includes("install"),
  );
  const hasBlockingReport = installReports.some((report) =>
    ["blocked", "error"].includes(report.status),
  );
  const installCompletedSuccessfully =
    startkit.complete &&
    (startkit.finalStatus === "complete" || startkit.finalStatus === "needs_input");
  const canContinueFromInstall =
    installCompletedSuccessfully ||
    (hasScanned && !installReportsRunning && !hasRunnableInstallWork && !hasBlockingReport);
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
    if (activeStep === "im") {
      setEnabledChannels(new Set());
      setActiveStep("remote");
    } else if (activeStep === "remote") {
      setTunnelProvider("none");
      setActiveStep("install");
    }
  }, [activeStep]);

  const rerunInstallScan = useCallback(() => {
    checkedInstallScanSignaturesRef.current.clear();
    checkedAgentLocalSignaturesRef.current.clear();
    checkedTunnelSignaturesRef.current.clear();
    setAgentInstallReports([]);
    setTunnelReports([]);
    void startkit.scan(finalSettings, choices);
  }, [choices, finalSettings, startkit]);

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
      if (installReportsRunning) {
        return {
          label: t("Checking..."),
          icon: <Loader2 className="h-4 w-4 animate-spin" />,
          disabled: true,
          run: () => {},
        };
      }
      if (canContinueFromInstall) {
        return {
          label: t("Continue"),
          icon: <ArrowRight className="h-4 w-4" />,
          disabled: false,
          run: () => setActiveStep("configure"),
        };
      }
      if (hasBlockingReport) {
        if (hasRunnableInstallWork) {
          return {
            label: t("Install anyway"),
            icon: <Download className="h-4 w-4" />,
            disabled: installReportsRunning,
            run: () =>
              void startkit.start(finalSettings, settingsPatch, choices, installReports),
          };
        }
        return {
          label: t("Check again"),
          icon: <RefreshCw className="h-4 w-4" />,
          disabled: !hasScanned,
          run: rerunInstallScan,
        };
      }
      if (hasRunnableInstallWork) {
        return {
          label: t("Install selected"),
          icon: <Download className="h-4 w-4" />,
          disabled: installReportsRunning,
          run: () =>
            void startkit.start(finalSettings, settingsPatch, choices, installReports),
        };
      }
      if (!hasRunnableInstallWork) {
        return {
          label: t("Check again"),
          icon: <RefreshCw className="h-4 w-4" />,
          disabled: !hasScanned,
          run: rerunInstallScan,
        };
      }
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
      disabled: activeStep === "agents" && enabledAgents.size === 0,
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
    hasRunnableInstallWork,
    hasScanned,
    hasBlockingReport,
    installCompletedSuccessfully,
    installReports,
    installReportsRunning,
    enabledAgents,
    rerunInstallScan,
    startkit,
    t,
  ]);

  const secondaryAction = useMemo<FooterAction | null>(() => {
    if (
      activeStep !== "install" ||
      startkit.running ||
      installReportsRunning ||
      canContinueFromInstall ||
      !hasBlockingReport ||
      !hasRunnableInstallWork
    ) {
      return null;
    }
    return {
      label: t("Check again"),
      icon: <RefreshCw className="h-4 w-4" />,
      disabled: !hasScanned,
      run: rerunInstallScan,
    };
  }, [
    activeStep,
    canContinueFromInstall,
    hasBlockingReport,
    hasRunnableInstallWork,
    hasScanned,
    installReportsRunning,
    rerunInstallScan,
    startkit.running,
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
            toolchainMode={toolchainMode}
            portableToolchain={portableToolchain}
            onDownloadSource={setDownloadSource}
            onToolchainMode={setToolchainMode}
            onPortableToolchain={setPortableToolchain}
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
          reportsById={agentReportsById}
          scanning={startkit.scanning || installReportsRunning}
          onToggleAgent={toggleAgent}
          pluginRegistry={pluginRegistry}
          discoveredPlugins={discoveredPlugins}
          pluginReports={pluginUpdateReports}
          enabledChannels={enabledChannels}
          onToggleChannel={toggleChannel}
          tunnels={tunnels}
          tunnelProvider={tunnelProvider}
          tunnelReports={tunnelReports}
          onTunnelProvider={setTunnelProvider}
          groupedReports={groupedReports}
          reports={installReports}
          running={startkit.running}
          complete={startkit.complete}
          finalStatus={startkit.finalStatus}
          startkitError={startkit.error}
          choices={choices}
          channelConfigs={channelConfigs}
          channelVerbose={channelVerbose}
          installingPlugins={installingPlugins}
          authStates={authStates}
          ngrokToken={ngrokToken}
          ngrokDomain={ngrokDomain}
          cfToken={cfToken}
          cfHostname={cfHostname}
          finishError={finishError}
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
        secondaryAction={secondaryAction}
        onBack={goBack}
        onSkip={skipStep}
        onCancel={() => void startkit.cancel()}
      />
    </div>
  );
}
