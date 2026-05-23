import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ChevronLeft, ChevronRight, Rocket } from "lucide-react";
import { useI18n } from "@va/i18n";

import { Button } from "@/components/ui/button";
import { LanguageMenu } from "@/components/LanguageMenu";
import { cn } from "@/lib/utils";

import { STEPS } from "./constants";
import { StepAgents } from "./components/StepAgents";
import { StepChannels } from "./components/StepChannels";
import { StepConfirm } from "./components/StepConfirm";
import { StepTunnel } from "./components/StepTunnel";
import { StepWelcome } from "./components/StepWelcome";
import { useChannelAuth } from "./hooks/useChannelAuth";
import { useInstallFlow } from "./hooks/useInstallFlow";
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
  TunnelSummary,
} from "./types";
import type {
  AgentId,
  OnboardingGoal,
  OnboardingStep,
  TunnelProvider,
} from "./constants";

const DEFAULT_ENABLED_AGENT_IDS = new Set<AgentId>(["claude", "codex"]);
const AGENT_DISPLAY_ORDER = [
  "claude",
  "codex",
  "gemini",
  "opencode",
  "cursor",
  "kiro",
  "qwen-code",
];
const STEP_GOALS: Partial<Record<OnboardingStep, OnboardingGoal>> = {
  "Quick Launch": "agents",
  Channels: "channels",
  Tunnel: "tunnel",
};

function orderAgents(agentDefs: AgentSummary[]): AgentSummary[] {
  const rank = new Map(AGENT_DISPLAY_ORDER.map((id, index) => [id, index]));
  return [...agentDefs].sort(
    (a, b) => (rank.get(a.id) ?? 999) - (rank.get(b.id) ?? 999),
  );
}

function visibleStepsForGoals(
  selectedGoals: Set<OnboardingGoal>,
): OnboardingStep[] {
  return STEPS.filter((candidate) => {
    const goal = STEP_GOALS[candidate];
    return !goal || selectedGoals.has(goal);
  });
}

export default function Onboarding() {
  const { t } = useI18n();
  const isMacTitlebar =
    typeof navigator !== "undefined" && /Mac/.test(navigator.platform);
  const [step, setStep] = useState(0);
  const [selectedGoals, setSelectedGoals] = useState<Set<OnboardingGoal>>(
    () => new Set<OnboardingGoal>(),
  );
  const [settings, setSettings] = useState<Settings>({});
  const [discoveredPlugins, setDiscoveredPlugins] = useState<
    DiscoveredChannelPlugin[]
  >([]);
  const [loaded, setLoaded] = useState(false);
  const [catalog, setCatalog] = useState<CatalogEntry[]>([]);
  const [profiles, setProfiles] = useState<ProfileSummary[]>([]);
  const [profileEditorOpen, setProfileEditorOpen] = useState(false);

  // Resource data from backend
  const [agents, setAgents] = useState<AgentSummary[]>([]);
  const [tunnels, setTunnels] = useState<TunnelSummary[]>([]);
  const [pluginRegistry, setPluginRegistry] = useState<PluginRegistryEntry[]>(
    [],
  );

  // Agents
  const [enabledAgents, setEnabledAgents] = useState<Set<AgentId>>(new Set());
  // Channels
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

  // Tunnel
  const [tunnelProvider, setTunnelProvider] =
    useState<TunnelProvider>("cloudflare");
  const [ngrokToken, setNgrokToken] = useState("");
  const [ngrokDomain, setNgrokDomain] = useState("");
  const [cfToken, setCfToken] = useState("");
  const [cfHostname, setCfHostname] = useState("");

  const visibleSteps = useMemo(
    () => visibleStepsForGoals(selectedGoals),
    [selectedGoals],
  );
  const currentStep =
    visibleSteps[Math.min(step, visibleSteps.length - 1)] ?? "Goals";

  useEffect(() => {
    setStep((previous) => Math.min(previous, visibleSteps.length - 1));
  }, [visibleSteps.length]);

  // ---- Load existing settings + resources ----
  useEffect(() => {
    Promise.all([
      invoke<Settings>("get_settings"),
      invoke<DiscoveredChannelPlugin[]>("list_channel_plugins"),
      invoke<AgentSummary[]>("list_agents"),
      invoke<TunnelSummary[]>("list_tunnels"),
      invoke<PluginRegistryEntry[]>("list_plugin_registry"),
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
          catalogDefs,
          profileDefs,
        ]) => {
          const orderedAgents = orderAgents(agentDefs);
          setSettings(loadedSettings);
          setDiscoveredPlugins(plugins);
          setAgents(orderedAgents);
          setTunnels(tunnelDefs);
          setPluginRegistry(pluginDefs);
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
      .catch(() => setLoaded(true));
  }, []);

  // ---- Channel handlers ----
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

  // ---- Auth flow + install orchestration (extracted hooks) ----
  const { authStates, startAuth, cancelAuth } = useChannelAuth({
    currentStep,
    discoveredPlugins,
    channelConfigs,
    onConfigChange: updateChannelConfig,
  });

  const {
    finishing,
    isInstalling,
    installComplete,
    installTasks,
    startInstall,
    cancelInstall,
    completeInstall,
  } = useInstallFlow();

  const handleFinish = useCallback(() => {
    const configureAgents = selectedGoals.has("agents");
    const configureChannels = selectedGoals.has("channels");
    const configureTunnel = selectedGoals.has("tunnel");
    const installScope = {
      agents: configureAgents,
      channels: configureChannels,
    };
    const finalSettings = buildSettings({
      settings,
      configureAgents,
      configureChannels,
      configureTunnel,
      enabledAgents,
      enabledChannels,
      registryPluginIds: new Set(pluginRegistry.map((plugin) => plugin.id)),
      channelConfigs,
      channelVerbose,
      discoveredPlugins,
      tunnelProvider,
      ngrokToken,
      ngrokDomain,
      cfToken,
      cfHostname,
    });
    void startInstall(finalSettings, installScope);
  }, [
    settings,
    selectedGoals,
    enabledAgents,
    enabledChannels,
    channelConfigs,
    channelVerbose,
    discoveredPlugins,
    tunnelProvider,
    ngrokToken,
    ngrokDomain,
    cfToken,
    cfHostname,
    startInstall,
  ]);

  const toggleAgent = useCallback((id: AgentId) => {
    setEnabledAgents((previous) => {
      const next = new Set(previous);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }, []);

  const toggleGoal = useCallback((goal: OnboardingGoal) => {
    setSelectedGoals((previous) => {
      const next = new Set(previous);
      if (next.has(goal)) {
        next.delete(goal);
      } else {
        next.add(goal);
      }
      return next;
    });
  }, []);

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

  if (!loaded) {
    return (
      <div className="flex items-center justify-center h-full">
        <span className="text-sm text-muted-foreground animate-pulse">
          {t("Loading…")}
        </span>
      </div>
    );
  }

  const isLast = step === visibleSteps.length - 1;

  return (
    <div className="flex flex-col h-full bg-background">
      <div
        className={cn(
          "relative flex h-12 items-center gap-3 pr-6",
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
        <div className="relative z-10 flex items-center gap-1 flex-1">
          {visibleSteps.map((label, index) => (
            <div key={label} className="flex items-center gap-1 flex-1">
              <div
                className={`h-1 flex-1 rounded-full transition-colors ${
                  index <= step ? "bg-primary" : "bg-border"
                }`}
              />
            </div>
          ))}
        </div>
        <div className="relative z-10">
          <LanguageMenu />
        </div>
      </div>
      <div className="px-6 pb-3">
        <span className="text-[10px] text-muted-foreground font-mono uppercase tracking-wider">
          {t("Step {{current}} of {{total}} — {{step}}", {
            current: step + 1,
            total: visibleSteps.length,
            step: t(currentStep),
          })}
        </span>
      </div>

      <div className="flex-1 overflow-y-auto px-6 pb-4">
        {currentStep === "Goals" && (
          <StepWelcome
            selectedGoals={selectedGoals}
            onToggleGoal={toggleGoal}
          />
        )}
        {currentStep === "Quick Launch" && (
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
        )}
        {currentStep === "Channels" && (
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
          />
        )}
        {currentStep === "Tunnel" && (
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
        )}
        {currentStep === "Confirm" && (
          <StepConfirm
            agents={agents}
            tunnels={tunnels}
            pluginRegistry={pluginRegistry}
            selectedGoals={selectedGoals}
            enabledAgents={enabledAgents}
            tunnelProvider={tunnelProvider}
            enabledChannels={enabledChannels}
            isInstalling={isInstalling}
            installComplete={installComplete}
            installTasks={installTasks}
          />
        )}
      </div>

      <div className="flex items-center justify-between px-6 py-4 border-t border-border shrink-0">
        {isInstalling ? (
          <>
            <div />
            {installComplete ? (
              <Button onClick={completeInstall}>
                <Rocket className="w-4 h-4" />
                {installTasks.some(
                  (task) =>
                    task.status === "error" || task.status === "cancelled",
                )
                  ? t("Continue Anyway")
                  : t("Open VibeAround")}
              </Button>
            ) : (
              <Button onClick={cancelInstall} variant="outline">
                {t("Cancel")}
              </Button>
            )}
          </>
        ) : (
          <>
            <Button
              onClick={() => setStep((v) => Math.max(0, v - 1))}
              disabled={step === 0}
              variant="ghost"
            >
              <ChevronLeft className="w-4 h-4" />
              {t("Back")}
            </Button>
            {isLast ? (
              <Button onClick={handleFinish} disabled={finishing}>
                {finishing ? (
                  <>{t("Confirming…")}</>
                ) : (
                  <>
                    <Rocket className="w-4 h-4" />
                    {t("Confirm")}
                  </>
                )}
              </Button>
            ) : (
              <Button
                onClick={() =>
                  setStep((v) => Math.min(visibleSteps.length - 1, v + 1))
                }
              >
                {currentStep === "Goals" ? t("Get Started") : t("Next")}
                <ChevronRight className="w-4 h-4" />
              </Button>
            )}
          </>
        )}
      </div>

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
