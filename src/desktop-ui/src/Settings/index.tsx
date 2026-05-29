import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Bot,
  Globe,
  MessageSquare,
  Network,
  RotateCw,
  Settings as SettingsIcon,
  Trash2,
  WandSparkles,
} from "lucide-react";
import { useI18n } from "@va/i18n";

import { StepChannels } from "../Onboarding/components/StepChannels";
import { StepTunnel } from "../Onboarding/components/StepTunnel";
import { useChannelAuth } from "../Onboarding/hooks/useChannelAuth";
import type { TunnelProvider } from "../Onboarding/constants";
import type {
  AgentSummary,
  ChannelVerboseConfig,
  ConfigSchemaProperty,
  DiscoveredChannelPlugin,
  PluginRegistryEntry,
  Settings as AppSettings,
  TunnelSummary,
} from "../Onboarding/types";
import { apiFetch } from "../lib/api";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { BrandIcon } from "@/components/brand-icon";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { StatusBanner } from "@/components/page";

interface SettingsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onServicesRestarted?: () => void;
}

type Notice = {
  variant: "success" | "warning" | "error";
  message: string;
};

type SaveState =
  | "idle"
  | "agents"
  | "proxy"
  | "im"
  | "tunnel"
  | "tunnel-restart"
  | "uninstall-mcp"
  | "uninstall-skills"
  | "restart-services";

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

export function SettingsDialog({
  open,
  onOpenChange,
  onServicesRestarted,
}: SettingsDialogProps) {
  const { t } = useI18n();
  const [settings, setSettings] = useState<AppSettings>({});
  const [agents, setAgents] = useState<AgentSummary[]>([]);
  const [pluginRegistry, setPluginRegistry] = useState<PluginRegistryEntry[]>([]);
  const [discoveredPlugins, setDiscoveredPlugins] = useState<
    DiscoveredChannelPlugin[]
  >([]);
  const [tunnels, setTunnels] = useState<TunnelSummary[]>([]);
  const [enabledAgents, setEnabledAgents] = useState<Set<string>>(
    () => new Set(),
  );
  const [enabledChannels, setEnabledChannels] = useState<Set<string>>(
    () => new Set(),
  );
  const [mcpAutoInstall, setMcpAutoInstall] = useState(true);
  const [skillAutoInstall, setSkillAutoInstall] = useState(true);
  const [channelConfigs, setChannelConfigs] = useState<
    Record<string, Record<string, string>>
  >({});
  const [channelVerbose, setChannelVerbose] = useState<
    Record<string, ChannelVerboseConfig>
  >({});
  const [tunnelProvider, setTunnelProvider] =
    useState<TunnelProvider>("none");
  const [proxyEnabled, setProxyEnabled] = useState(false);
  const [proxyHttp, setProxyHttp] = useState("");
  const [proxyNoProxy, setProxyNoProxy] = useState("");
  const [ngrokToken, setNgrokToken] = useState("");
  const [ngrokDomain, setNgrokDomain] = useState("");
  const [cfToken, setCfToken] = useState("");
  const [cfHostname, setCfHostname] = useState("");
  const [installingPlugins, setInstallingPlugins] = useState<Set<string>>(
    () => new Set(),
  );
  const [loading, setLoading] = useState(true);
  const [settingsLoaded, setSettingsLoaded] = useState(false);
  const [saving, setSaving] = useState<SaveState>("idle");
  const [notice, setNotice] = useState<Notice | null>(null);

  const hydrateChannels = useCallback(
    (
      loadedSettings: AppSettings,
      registry: PluginRegistryEntry[],
      discovered: DiscoveredChannelPlugin[],
    ) => {
      const knownIds = new Set([
        ...registry.map((plugin) => plugin.id),
        ...discovered.map((plugin) => plugin.id),
      ]);
      const channels = isRecord(loadedSettings.channels)
        ? loadedSettings.channels
        : {};
      const enabled = new Set<string>();
      const configs: Record<string, Record<string, string>> = {};
      const verbose: Record<string, ChannelVerboseConfig> = {};

      for (const [id, channelConfig] of Object.entries(channels)) {
        if (!knownIds.has(id) || !isRecord(channelConfig)) continue;
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
    },
    [],
  );

  const hydrateAgents = useCallback(
    (loadedSettings: AppSettings, loadedAgents: AgentSummary[]) => {
      const knownIds = new Set(loadedAgents.map((agent) => agent.id));
      const enabled = Array.isArray(loadedSettings.enabled_agents)
        ? loadedSettings.enabled_agents
            .filter((id): id is string => typeof id === "string")
            .filter((id) => knownIds.has(id))
        : loadedAgents.map((agent) => agent.id);
      setEnabledAgents(new Set(enabled));
    },
    [],
  );

  const hydrateTunnel = useCallback((loadedSettings: AppSettings) => {
    const tunnel = loadedSettings.tunnel;
    setTunnelProvider(tunnel?.provider ?? "none");
    setNgrokToken(tunnel?.ngrok?.auth_token ?? "");
    setNgrokDomain(tunnel?.ngrok?.domain ?? "");
    setCfToken(tunnel?.cloudflare?.tunnel_token ?? "");
    setCfHostname(tunnel?.cloudflare?.hostname ?? "");
  }, []);

  const hydrateProxy = useCallback((loadedSettings: AppSettings) => {
    const proxy = loadedSettings.proxy;
    setProxyEnabled(Boolean(proxy?.enabled ?? proxy?.http_proxy));
    setProxyHttp(proxy?.http_proxy ?? "");
    setProxyNoProxy(proxy?.no_proxy ?? "");
  }, []);

  const hydrateIntegrations = useCallback((loadedSettings: AppSettings) => {
    const integrations = loadedSettings.integrations;
    setMcpAutoInstall(integrations?.mcp_auto_install ?? true);
    setSkillAutoInstall(integrations?.skill_auto_install ?? true);
  }, []);

  const load = useCallback(async () => {
    setLoading(true);
    setSettingsLoaded(false);
    setNotice(null);
    try {
      const [loadedSettings, agentDefs, registry, discovered, tunnelDefs] =
        await Promise.all([
          invoke<AppSettings>("get_settings"),
          invoke<AgentSummary[]>("list_agents"),
          invoke<PluginRegistryEntry[]>("list_plugin_registry"),
          invoke<DiscoveredChannelPlugin[]>("list_channel_plugins"),
          invoke<TunnelSummary[]>("list_tunnels"),
        ]);
      const orderedAgents = orderAgents(agentDefs);
      setSettings(loadedSettings);
      setAgents(orderedAgents);
      setPluginRegistry(registry);
      setDiscoveredPlugins(discovered);
      setTunnels(tunnelDefs);
      hydrateAgents(loadedSettings, orderedAgents);
      hydrateChannels(loadedSettings, registry, discovered);
      hydrateTunnel(loadedSettings);
      hydrateProxy(loadedSettings);
      hydrateIntegrations(loadedSettings);
      setSettingsLoaded(true);
    } catch (error) {
      setNotice({
        variant: "error",
        message: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setLoading(false);
    }
  }, [
    hydrateAgents,
    hydrateChannels,
    hydrateIntegrations,
    hydrateProxy,
    hydrateTunnel,
  ]);

  useEffect(() => {
    if (open) void load();
  }, [open, load]);

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

  const toggleAgent = useCallback((agentId: string) => {
    setEnabledAgents((prev) => {
      const next = new Set(prev);
      if (next.has(agentId)) next.delete(agentId);
      else next.add(agentId);
      return next;
    });
  }, []);

  const installPlugin = useCallback(
    async (pluginId: string, githubUrl: string) => {
      setInstallingPlugins((prev) => new Set(prev).add(pluginId));
      setNotice(null);
      try {
        await invoke("install_plugin", { request: { pluginId, githubUrl } });
        const plugins = await invoke<DiscoveredChannelPlugin[]>(
          "list_channel_plugins",
        );
        setDiscoveredPlugins(plugins);
      } catch (error) {
        setNotice({
          variant: "error",
          message: error instanceof Error ? error.message : String(error),
        });
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
    active: open,
    discoveredPlugins,
    channelConfigs,
    onConfigChange: updateChannelConfig,
  });

  const canSubmit = useMemo(
    () => settingsLoaded && !loading && saving === "idle",
    [settingsLoaded, loading, saving],
  );

  const restartServices = useCallback(async () => {
    setSaving("restart-services");
    setNotice(null);
    try {
      await invoke("restart_services");
      onServicesRestarted?.();
      setNotice({ variant: "success", message: "Services restarted." });
    } catch (error) {
      setNotice({
        variant: "error",
        message: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setSaving("idle");
    }
  }, [onServicesRestarted]);

  const applyAgentSettings = useCallback(async () => {
    setSaving("agents");
    setNotice(null);
    try {
      const nextSettings = buildAgentSettings({
        settings,
        agents,
        enabledAgents,
        mcpAutoInstall,
        skillAutoInstall,
      });
      await invoke("save_settings", { settings: nextSettings });
      setSettings(nextSettings);
      const response = await apiFetch("/api/settings/reload", { method: "POST" });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      onServicesRestarted?.();
      setNotice({ variant: "success", message: "Agent settings applied." });
    } catch (error) {
      setNotice({
        variant: "error",
        message: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setSaving("idle");
    }
  }, [
    settings,
    agents,
    enabledAgents,
    mcpAutoInstall,
    skillAutoInstall,
    onServicesRestarted,
  ]);

  const applyProxySettings = useCallback(async () => {
    setSaving("proxy");
    setNotice(null);
    try {
      const nextSettings = buildProxySettings({
        settings,
        proxyEnabled,
        proxyHttp,
        proxyNoProxy,
      });
      await invoke("save_settings", { settings: nextSettings });
      setSettings(nextSettings);
      const response = await apiFetch("/api/settings/reload", { method: "POST" });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      onServicesRestarted?.();
      setNotice({ variant: "success", message: "Proxy settings applied." });
    } catch (error) {
      setNotice({
        variant: "error",
        message: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setSaving("idle");
    }
  }, [settings, proxyEnabled, proxyHttp, proxyNoProxy, onServicesRestarted]);

  const uninstallIntegrations = useCallback(
    async (kind: "mcp" | "skills") => {
      setSaving(kind === "mcp" ? "uninstall-mcp" : "uninstall-skills");
      setNotice(null);
      try {
        await invoke("uninstall_agent_integrations", {
          removeMcp: kind === "mcp",
          removeSkills: kind === "skills",
        });
        setNotice({
          variant: "success",
          message:
            kind === "mcp"
              ? "Legacy VibeAround MCP entries removed."
              : "Legacy VibeAround skill files removed.",
        });
      } catch (error) {
        setNotice({
          variant: "error",
          message: error instanceof Error ? error.message : String(error),
        });
      } finally {
        setSaving("idle");
      }
    },
    [],
  );

  const applyImSettings = useCallback(async () => {
    setSaving("im");
    setNotice(null);
    try {
      const nextSettings = buildChannelSettings({
        settings,
        pluginRegistry,
        discoveredPlugins,
        enabledChannels,
        channelConfigs,
        channelVerbose,
      });
      await invoke("save_settings", { settings: nextSettings });
      setSettings(nextSettings);
      const response = await apiFetch("/api/channels/sync", { method: "POST" });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      onServicesRestarted?.();
      setNotice({ variant: "success", message: "IM settings applied." });
    } catch (error) {
      setNotice({
        variant: "error",
        message: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setSaving("idle");
    }
  }, [
    settings,
    pluginRegistry,
    discoveredPlugins,
    enabledChannels,
    channelConfigs,
    channelVerbose,
    onServicesRestarted,
  ]);

  const saveTunnelSettings = useCallback(
    async (restart: boolean) => {
      setSaving(restart ? "tunnel-restart" : "tunnel");
      setNotice(null);
      try {
        const nextSettings = buildTunnelSettings({
          settings,
          tunnelProvider,
          ngrokToken,
          ngrokDomain,
          cfToken,
          cfHostname,
        });
        await invoke("save_settings", { settings: nextSettings });
        setSettings(nextSettings);

        if (restart) {
          await invoke("restart_services");
          onServicesRestarted?.();
          setNotice({ variant: "success", message: "Tunnel settings applied." });
        } else {
          setNotice({ variant: "success", message: "Tunnel settings saved." });
        }
      } catch (error) {
        setNotice({
          variant: "error",
          message: error instanceof Error ? error.message : String(error),
        });
      } finally {
        setSaving("idle");
      }
    },
    [
      settings,
      tunnelProvider,
      ngrokToken,
      ngrokDomain,
      cfToken,
      cfHostname,
      onServicesRestarted,
    ],
  );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="!flex h-[680px] min-h-[520px] w-[min(860px,calc(100vw-32px))] max-h-[calc(100vh-64px)] max-w-[calc(100vw-32px)] overflow-hidden p-0 sm:max-w-[min(860px,calc(100vw-32px))]">
        <Tabs orientation="vertical" defaultValue="general" className="min-h-0 flex-1 gap-0">
          <aside className="flex w-44 shrink-0 flex-col border-r border-border bg-muted/20 px-4 py-4">
            <DialogHeader className="mb-4 pr-8">
              <DialogTitle className="flex items-center gap-2 text-base">
                <SettingsIcon className="h-4 w-4 text-primary" />
                {t("Settings")}
              </DialogTitle>
            </DialogHeader>
            <TabsList className="!h-auto w-full flex-col items-stretch justify-start gap-1 rounded-none bg-transparent p-0">
              <TabsTrigger
                value="general"
                className="!h-8 w-full justify-start gap-2 px-2 text-sm data-[state=active]:border-transparent data-[state=active]:bg-primary/10 data-[state=active]:text-primary data-[state=active]:shadow-none [&_svg:not([class*='size-'])]:!size-3.5"
              >
                <SettingsIcon className="h-3 w-3" />
                {t("General")}
              </TabsTrigger>
              <TabsTrigger
                value="agents"
                className="!h-8 w-full justify-start gap-2 px-2 text-sm data-[state=active]:border-transparent data-[state=active]:bg-primary/10 data-[state=active]:text-primary data-[state=active]:shadow-none [&_svg:not([class*='size-'])]:!size-3.5"
              >
                <Bot className="h-3 w-3" />
                {t("Agents")}
              </TabsTrigger>
              <TabsTrigger
                value="im"
                className="!h-8 w-full justify-start gap-2 px-2 text-sm data-[state=active]:border-transparent data-[state=active]:bg-primary/10 data-[state=active]:text-primary data-[state=active]:shadow-none [&_svg:not([class*='size-'])]:!size-3.5"
              >
                <MessageSquare className="h-3 w-3" />
                {t("IM")}
              </TabsTrigger>
              <TabsTrigger
                value="tunnel"
                className="!h-8 w-full justify-start gap-2 px-2 text-sm data-[state=active]:border-transparent data-[state=active]:bg-primary/10 data-[state=active]:text-primary data-[state=active]:shadow-none [&_svg:not([class*='size-'])]:!size-3.5"
              >
                <Globe className="h-3 w-3" />
                {t("Tunnel")}
              </TabsTrigger>
              <TabsTrigger
                value="proxy"
                className="!h-8 w-full justify-start gap-2 px-2 text-sm data-[state=active]:border-transparent data-[state=active]:bg-primary/10 data-[state=active]:text-primary data-[state=active]:shadow-none [&_svg:not([class*='size-'])]:!size-3.5"
              >
                <Network className="h-3 w-3" />
                {t("Proxy")}
              </TabsTrigger>
            </TabsList>
          </aside>

          <div className="flex min-w-0 flex-1 flex-col">
            <TabsContent
              value="general"
              className="min-h-0 overflow-hidden data-[state=active]:flex data-[state=active]:flex-col"
            >
              <div className="min-h-0 flex-1 overflow-y-auto px-5 py-5 [scrollbar-gutter:stable]">
                <div className="mb-5">
                  <h2 className="flex items-center gap-2 text-base font-semibold">
                    <SettingsIcon className="h-4 w-4 text-primary" />
                    {t("General")}
                  </h2>
                  <p className="mt-1 text-xs text-muted-foreground">
                    {t("Manage local service controls and rerun setup when needed.")}
                  </p>
                  <SettingsNotice notice={notice} />
                </div>
                <div className="rounded-md border border-border">
                  <SettingsActionRow
                    label={t("Restart Services")}
                    description={t("Restart VibeAround runtime services after local changes.")}
                    action={
                      <Button
                        type="button"
                        size="sm"
                        className="text-xs"
                        disabled={saving !== "idle"}
                        onClick={() => void restartServices()}
                      >
                        <RotateCw className="h-3 w-3" />
                        {saving === "restart-services"
                          ? t("Restarting services…")
                          : t("Restart")}
                      </Button>
                    }
                  />
                  <SettingsActionRow
                    label={t("Rerun Onboarding")}
                    description={t("Open the configuration wizard again.")}
                    action={
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        className="text-xs"
                        disabled={saving !== "idle"}
                        onClick={() => window.location.replace("/onboarding")}
                      >
                        <WandSparkles className="h-3 w-3" />
                        {t("Open Config Wizard")}
                      </Button>
                    }
                  />
                </div>
              </div>
            </TabsContent>

            <TabsContent
              value="im"
              className="min-h-0 overflow-hidden data-[state=active]:flex data-[state=active]:flex-col"
            >
              {loading ? (
                <div className="min-h-0 flex-1 overflow-y-auto px-5 py-5 [scrollbar-gutter:stable]">
                  <LoadingBlock />
                </div>
              ) : (
                <>
                  <div className="min-h-0 flex-1 overflow-y-auto px-5 py-5 [scrollbar-gutter:stable]">
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
                      onStartAuth={(pluginId) => void startAuth(pluginId)}
                      onCancelAuth={(pluginId) => void cancelAuth(pluginId)}
                      switchSize="sm"
                      notice={<SettingsNotice notice={notice} />}
                    />
                  </div>
                  <div className="flex shrink-0 justify-end border-t border-border px-5 py-3">
                    <Button
                      type="button"
                      size="sm"
                      disabled={!canSubmit}
                      onClick={() => void applyImSettings()}
                    >
                      {saving === "im" ? t("Applying…") : t("Apply IM Settings")}
                    </Button>
                  </div>
                </>
              )}
            </TabsContent>

            <TabsContent
              value="agents"
              className="min-h-0 overflow-hidden data-[state=active]:flex data-[state=active]:flex-col"
            >
              {loading ? (
                <div className="min-h-0 flex-1 overflow-y-auto px-5 py-5 [scrollbar-gutter:stable]">
                  <LoadingBlock />
                </div>
              ) : (
                <>
                  <div className="min-h-0 flex-1 overflow-y-auto px-5 py-5 [scrollbar-gutter:stable]">
                    <AgentSettingsPanel
                      agents={agents}
                      enabledAgents={enabledAgents}
                      mcpAutoInstall={mcpAutoInstall}
                      skillAutoInstall={skillAutoInstall}
                      onToggle={toggleAgent}
                      onMcpAutoInstallChange={setMcpAutoInstall}
                      onSkillAutoInstallChange={setSkillAutoInstall}
                      onUninstallMcp={() => void uninstallIntegrations("mcp")}
                      onUninstallSkills={() => void uninstallIntegrations("skills")}
                      saving={saving}
                      notice={<SettingsNotice notice={notice} />}
                    />
                  </div>
                  <div className="flex shrink-0 justify-end border-t border-border px-5 py-3">
                    <Button
                      type="button"
                      size="sm"
                      disabled={!canSubmit}
                      onClick={() => void applyAgentSettings()}
                    >
                      {saving === "agents"
                        ? t("Applying…")
                        : t("Apply Agent Settings")}
                    </Button>
                  </div>
                </>
              )}
            </TabsContent>

            <TabsContent
              value="proxy"
              className="min-h-0 overflow-hidden data-[state=active]:flex data-[state=active]:flex-col"
            >
              {loading ? (
                <div className="min-h-0 flex-1 overflow-y-auto px-5 py-5 [scrollbar-gutter:stable]">
                  <LoadingBlock />
                </div>
              ) : (
                <>
                  <div className="min-h-0 flex-1 overflow-y-auto px-5 py-5 [scrollbar-gutter:stable]">
                    <ProxySettingsPanel
                      proxyEnabled={proxyEnabled}
                      proxyHttp={proxyHttp}
                      proxyNoProxy={proxyNoProxy}
                      onProxyEnabledChange={setProxyEnabled}
                      onProxyHttpChange={setProxyHttp}
                      onProxyNoProxyChange={setProxyNoProxy}
                      notice={<SettingsNotice notice={notice} />}
                    />
                  </div>
                  <div className="flex shrink-0 justify-end border-t border-border px-5 py-3">
                    <Button
                      type="button"
                      size="sm"
                      disabled={!canSubmit}
                      onClick={() => void applyProxySettings()}
                    >
                      {saving === "proxy"
                        ? t("Applying…")
                        : t("Apply Proxy Settings")}
                    </Button>
                  </div>
                </>
              )}
            </TabsContent>

            <TabsContent
              value="tunnel"
              className="min-h-0 overflow-hidden data-[state=active]:flex data-[state=active]:flex-col"
            >
              {loading ? (
                <div className="min-h-0 flex-1 overflow-y-auto px-5 py-5 [scrollbar-gutter:stable]">
                  <LoadingBlock />
                </div>
              ) : (
                <>
                  <div className="min-h-0 flex-1 overflow-y-auto px-5 py-5 [scrollbar-gutter:stable]">
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
                      notice={<SettingsNotice notice={notice} />}
                    />
                  </div>
                  <div className="flex shrink-0 flex-wrap justify-end gap-2 border-t border-border px-5 py-3">
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      disabled={!canSubmit}
                      onClick={() => void saveTunnelSettings(false)}
                    >
                      {saving === "tunnel" ? t("Saving…") : t("Save")}
                    </Button>
                    <Button
                      type="button"
                      size="sm"
                      disabled={!canSubmit}
                      onClick={() => void saveTunnelSettings(true)}
                    >
                      <RotateCw className="h-3 w-3" />
                      {saving === "tunnel-restart"
                        ? t("Restarting services…")
                        : t("Save & Restart Services")}
                    </Button>
                  </div>
                </>
              )}
            </TabsContent>
          </div>
        </Tabs>
      </DialogContent>
    </Dialog>
  );
}

function AgentSettingsPanel({
  agents,
  enabledAgents,
  mcpAutoInstall,
  skillAutoInstall,
  onToggle,
  onMcpAutoInstallChange,
  onSkillAutoInstallChange,
  onUninstallMcp,
  onUninstallSkills,
  saving,
  notice,
}: {
  agents: AgentSummary[];
  enabledAgents: Set<string>;
  mcpAutoInstall: boolean;
  skillAutoInstall: boolean;
  onToggle: (agentId: string) => void;
  onMcpAutoInstallChange: (value: boolean) => void;
  onSkillAutoInstallChange: (value: boolean) => void;
  onUninstallMcp: () => void;
  onUninstallSkills: () => void;
  saving: SaveState;
  notice?: ReactNode;
}) {
  const { t } = useI18n();
  return (
    <div className="space-y-5">
      <div>
        <h2 className="flex items-center gap-2 text-base font-semibold">
          <Bot className="h-4 w-4 text-primary" />
          {t("Agents")}
        </h2>
        <p className="mt-1 text-xs text-muted-foreground">
          {t(
            "Choose which CLIs appear in Launch and new IM sessions. Running sessions continue.",
          )}
        </p>
        {notice}
      </div>
      <div className="grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-2">
        {agents.map((agent) => {
          const isEnabled = enabledAgents.has(agent.id);
          return (
            <button
              key={agent.id}
              type="button"
              role="checkbox"
              aria-checked={isEnabled}
              className={`relative flex min-h-12 items-center gap-2 rounded-md border p-2 pr-8 text-left transition-colors ${
                isEnabled
                  ? "border-primary/40 bg-primary/5"
                  : "border-border hover:border-border/80"
              }`}
              onClick={() => onToggle(agent.id)}
            >
              <BrandIcon
                kind="cli"
                id={agent.id}
                label={agent.display_name}
                className="h-6 w-6"
              />
              <span className="flex min-w-0 flex-1 items-center">
                <span
                  className={`truncate text-sm font-medium ${
                    isEnabled ? "text-foreground" : "text-muted-foreground"
                  }`}
                >
                  {agent.display_name}
                </span>
              </span>
              <Checkbox
                checked={isEnabled}
                aria-hidden="true"
                tabIndex={-1}
                className="pointer-events-none absolute right-2.5 top-1/2 -translate-y-1/2"
              />
            </button>
          );
        })}
      </div>
      {enabledAgents.size === 0 && (
        <StatusBanner variant="warning">
          {t(
            "No agents are enabled. Launch will stay hidden until at least one agent is selected.",
          )}
        </StatusBanner>
      )}
      <div className="rounded-md border border-border">
        <SettingsActionRow
          label={t("Auto-install MCP")}
          description={t("Install VibeAround MCP in the selected workspace when an agent launches.")}
          action={
            <Switch
              checked={mcpAutoInstall}
              onCheckedChange={onMcpAutoInstallChange}
              aria-label={t("Auto-install MCP")}
              size="sm"
            />
          }
        />
        <SettingsActionRow
          label={t("Auto-install skills")}
          description={t("Install VibeAround skills in the selected workspace when an agent launches.")}
          action={
            <Switch
              checked={skillAutoInstall}
              onCheckedChange={onSkillAutoInstallChange}
              aria-label={t("Auto-install skills")}
              size="sm"
            />
          }
        />
        <SettingsActionRow
          label={t("Uninstall legacy MCP")}
          description={t("Remove legacy VibeAround MCP entries from old global config.")}
          action={
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="text-xs"
              disabled={saving !== "idle"}
              onClick={onUninstallMcp}
            >
              <Trash2 className="h-3 w-3" />
              {saving === "uninstall-mcp" ? t("Removing…") : t("Remove")}
            </Button>
          }
        />
        <SettingsActionRow
          label={t("Uninstall legacy skill")}
          description={t("Remove legacy VibeAround skill files from old global folders.")}
          action={
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="text-xs"
              disabled={saving !== "idle"}
              onClick={onUninstallSkills}
            >
              <Trash2 className="h-3 w-3" />
              {saving === "uninstall-skills" ? t("Removing…") : t("Remove")}
            </Button>
          }
        />
      </div>
    </div>
  );
}

function ProxySettingsPanel({
  proxyEnabled,
  proxyHttp,
  proxyNoProxy,
  onProxyEnabledChange,
  onProxyHttpChange,
  onProxyNoProxyChange,
  notice,
}: {
  proxyEnabled: boolean;
  proxyHttp: string;
  proxyNoProxy: string;
  onProxyEnabledChange: (value: boolean) => void;
  onProxyHttpChange: (value: string) => void;
  onProxyNoProxyChange: (value: string) => void;
  notice?: ReactNode;
}) {
  const { t } = useI18n();
  return (
    <div className="space-y-5">
      <div>
        <h2 className="flex items-center gap-2 text-base font-semibold">
          <Network className="h-4 w-4 text-primary" />
          {t("Proxy")}
        </h2>
        <p className="mt-1 text-xs text-muted-foreground">
          {t(
            "Configure the HTTP proxy used by profile provider requests that opt in from profile settings.",
          )}
        </p>
        {notice}
      </div>
      <div className="rounded-md border border-border">
        <SettingsActionRow
          label={t("Enable Settings proxy")}
          description={t("Allow profiles to opt in to this HTTP proxy.")}
          action={
            <Switch
              checked={proxyEnabled}
              onCheckedChange={onProxyEnabledChange}
              aria-label={t("Enable Settings proxy")}
              size="sm"
            />
          }
        />
        <div className="grid gap-3 border-b border-border px-4 py-4 last:border-b-0">
          <label className="block">
            <span className="text-xs text-muted-foreground">
              {t("HTTP proxy URL")}
            </span>
            <Input
              type="text"
              value={proxyHttp}
              onChange={(event) => onProxyHttpChange(event.currentTarget.value)}
              placeholder="http://127.0.0.1:7890"
              className="mt-1"
            />
          </label>
        </div>
        <div className="grid gap-3 border-b border-border px-4 py-4 last:border-b-0">
          <label className="block">
            <span className="text-xs text-muted-foreground">
              {t("No proxy")}
            </span>
            <Input
              type="text"
              value={proxyNoProxy}
              onChange={(event) => onProxyNoProxyChange(event.currentTarget.value)}
              placeholder="localhost,127.0.0.1,::1"
              className="mt-1"
            />
          </label>
        </div>
      </div>
    </div>
  );
}

function SettingsNotice({ notice }: { notice: Notice | null }) {
  const { t } = useI18n();
  if (!notice) return null;
  return (
    <div className="mt-3">
      <StatusBanner variant={notice.variant}>{t(notice.message)}</StatusBanner>
    </div>
  );
}

function SettingsActionRow({
  label,
  description,
  action,
}: {
  label: string;
  description?: string;
  action: ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-3 border-b border-border px-4 py-4 last:border-b-0">
      <div className="min-w-0">
        <div className="text-sm font-medium">{label}</div>
        {description && (
          <div className="mt-0.5 text-xs text-muted-foreground">
            {description}
          </div>
        )}
      </div>
      {action}
    </div>
  );
}

function LoadingBlock() {
  const { t } = useI18n();
  return (
    <p className="px-1 py-6 text-center text-xs text-muted-foreground">
      {t("Loading…")}
    </p>
  );
}

function buildAgentSettings({
  settings,
  agents,
  enabledAgents,
  mcpAutoInstall,
  skillAutoInstall,
}: {
  settings: AppSettings;
  agents: AgentSummary[];
  enabledAgents: Set<string>;
  mcpAutoInstall: boolean;
  skillAutoInstall: boolean;
}): AppSettings {
  const result: AppSettings = { ...settings };
  result.enabled_agents = agents
    .map((agent) => agent.id)
    .filter((id) => enabledAgents.has(id));
  result.integrations = {
    ...(isRecord(settings.integrations) ? settings.integrations : {}),
    mcp_auto_install: mcpAutoInstall,
    skill_auto_install: skillAutoInstall,
  };
  return result;
}

function buildProxySettings({
  settings,
  proxyEnabled,
  proxyHttp,
  proxyNoProxy,
}: {
  settings: AppSettings;
  proxyEnabled: boolean;
  proxyHttp: string;
  proxyNoProxy: string;
}): AppSettings {
  const result: AppSettings = { ...settings };
  const proxy: NonNullable<AppSettings["proxy"]> = {};
  const trimmedHttp = proxyHttp.trim();
  const trimmedNoProxy = proxyNoProxy.trim();
  const hasValues = Boolean(trimmedHttp || trimmedNoProxy);
  proxy.enabled = proxyEnabled;
  if (trimmedHttp) proxy.http_proxy = trimmedHttp;
  if (trimmedNoProxy) proxy.no_proxy = trimmedNoProxy;
  if (proxyEnabled || hasValues) {
    result.proxy = proxy;
  } else {
    delete result.proxy;
  }
  return result;
}

function buildChannelSettings({
  settings,
  pluginRegistry,
  discoveredPlugins,
  enabledChannels,
  channelConfigs,
  channelVerbose,
}: {
  settings: AppSettings;
  pluginRegistry: PluginRegistryEntry[];
  discoveredPlugins: DiscoveredChannelPlugin[];
  enabledChannels: Set<string>;
  channelConfigs: Record<string, Record<string, string>>;
  channelVerbose: Record<string, ChannelVerboseConfig>;
}): AppSettings {
  const result: AppSettings = { ...settings };
  const existingChannels = isRecord(settings.channels) ? settings.channels : {};
  const knownPluginIds = new Set([
    ...pluginRegistry.map((plugin) => plugin.id),
    ...discoveredPlugins.map((plugin) => plugin.id),
  ]);
  const discoveredMap = new Map(
    discoveredPlugins.map((plugin) => [plugin.id, plugin]),
  );
  const channels: Record<string, Record<string, unknown>> = {};

  for (const [id, value] of Object.entries(existingChannels)) {
    if (!knownPluginIds.has(id) && isRecord(value)) {
      channels[id] = { ...value };
    }
  }

  for (const id of knownPluginIds) {
    if (!enabledChannels.has(id)) continue;
    const existing = isRecord(existingChannels[id])
      ? existingChannels[id]
      : {};
    const config: Record<string, unknown> = { ...existing };
    const schemaProps = discoveredMap.get(id)?.configSchema?.properties ?? {};
    const editableKeys = new Set([
      ...Object.entries(schemaProps)
        .filter(([, prop]) => !prop.hidden)
        .map(([key]) => key),
      ...Object.keys(channelConfigs[id] ?? {}),
    ]);

    for (const key of editableKeys) {
      if (key === "verbose") continue;
      const value = channelConfigs[id]?.[key] ?? "";
      const prop = schemaProps[key] as ConfigSchemaProperty | undefined;
      if (value || prop?.default) {
        config[key] = value || prop?.default;
      } else {
        delete config[key];
      }
    }

    const verbose = channelVerbose[id] ?? parseChannelVerbose(config.verbose);
    config.verbose = {
      show_thinking: verbose.show_thinking,
      show_tool_use: verbose.show_tool_use,
    };
    channels[id] = config;
  }

  if (Object.keys(channels).length > 0) {
    result.channels = channels;
  } else {
    delete result.channels;
  }

  return result;
}

function buildTunnelSettings({
  settings,
  tunnelProvider,
  ngrokToken,
  ngrokDomain,
  cfToken,
  cfHostname,
}: {
  settings: AppSettings;
  tunnelProvider: TunnelProvider;
  ngrokToken: string;
  ngrokDomain: string;
  cfToken: string;
  cfHostname: string;
}): AppSettings {
  const result: AppSettings = { ...settings };
  if (tunnelProvider === "none") {
    delete result.tunnel;
    return result;
  }

  const tunnel: AppSettings["tunnel"] = { provider: tunnelProvider };
  if (tunnelProvider === "ngrok") {
    tunnel.ngrok = {};
    if (ngrokToken.trim()) tunnel.ngrok.auth_token = ngrokToken.trim();
    if (ngrokDomain.trim()) tunnel.ngrok.domain = ngrokDomain.trim();
  }
  if (tunnelProvider === "cloudflare") {
    tunnel.cloudflare = {};
    if (cfToken.trim()) tunnel.cloudflare.tunnel_token = cfToken.trim();
    if (cfHostname.trim()) tunnel.cloudflare.hostname = cfHostname.trim();
  }
  result.tunnel = tunnel;
  return result;
}

function defaultChannelVerbose(): ChannelVerboseConfig {
  return {
    show_thinking: false,
    show_tool_use: false,
  };
}

function parseChannelVerbose(value: unknown): ChannelVerboseConfig {
  if (!isRecord(value)) return defaultChannelVerbose();
  return {
    show_thinking:
      typeof value.show_thinking === "boolean" ? value.show_thinking : false,
    show_tool_use:
      typeof value.show_tool_use === "boolean" ? value.show_tool_use : false,
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function orderAgents(agents: AgentSummary[]): AgentSummary[] {
  const rank = new Map(AGENT_DISPLAY_ORDER.map((id, index) => [id, index]));
  return [...agents].sort(
    (a, b) => (rank.get(a.id) ?? 999) - (rank.get(b.id) ?? 999),
  );
}
