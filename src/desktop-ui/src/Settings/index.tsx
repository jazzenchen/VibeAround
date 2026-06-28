import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDirectoryDialog } from "@tauri-apps/plugin-dialog";
import { WorkspacesResponseSchema } from "@va/client";
import {
  AlertCircle,
  Bot,
  CheckCircle2,
  Copy,
  Download,
  ExternalLink,
  FolderOpen,
  Globe,
  Loader2,
  MessageSquare,
  Network,
  Puzzle,
  RotateCw,
  Search,
  SlidersHorizontal,
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
  ToolchainMode,
  TunnelSummary,
} from "../Onboarding/types";
import { apiFetch } from "../lib/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
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
  initialTarget?: SettingsDialogTarget | null;
}

export type SettingsDialogTarget = {
  tab: string;
  pluginId?: string | null;
  nonce?: number;
};

type Notice = {
  variant: "success" | "warning" | "error";
  message: string;
};

type AuthFile = {
  token: string;
};

type SaveState =
  | "idle"
  | "agents"
  | "api-bridge"
  | "web-search"
  | "proxy"
  | "general"
  | "im"
  | "tunnel"
  | "tunnel-restart"
  | "uninstall-mcp"
  | "uninstall-skills"
  | "restart-services";

type ApiBridgeRetryFormState = {
  retry429Enabled: boolean;
  retry429MaxRetries: string;
  retry429Unlimited: boolean;
  retry429DelaySeconds: string;
};

type SearchSourceId = "exa" | "tavily" | "grok" | "brave";

type SearchSourceForm = {
  enabled: boolean;
  apiKey: string;
};

type SearchContextSize = "low" | "medium" | "high";

type ManagedPluginCategory = "im" | "acp" | "search";
type ManagedPluginStatus =
  | "ok"
  | "missing"
  | "outdated";

type ManagedPluginSummary = {
  category: ManagedPluginCategory;
  id: string;
  kind: string;
  name: string;
  description: string;
  status: ManagedPluginStatus;
  installed: boolean;
  installable: boolean;
  version?: string;
  latestVersion?: string;
  source?: DiscoveredChannelPlugin["source"];
  path?: string;
  github?: string;
  message?: string;
  actions: string[];
};

type TestSearchResult = {
  title: string;
  url: string;
  snippet: string;
  content: string;
  score?: number;
  publishedDate?: string;
  source: string;
};

type TestSearchResponse = {
  provider: string;
  query: string;
  results: TestSearchResult[];
  citations: string[];
};

const DEFAULT_SEARCH_MAX_RESULTS = "5";
const DEFAULT_SEARCH_CONTEXT_SIZE: SearchContextSize = "medium";
const SEARCH_MAX_RESULTS_MIN = 1;
const SEARCH_MAX_RESULTS_MAX = 20;

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

const SEARCH_SOURCE_DEFS: Array<{ id: SearchSourceId; label: string }> = [
  { id: "exa", label: "Exa" },
  { id: "tavily", label: "Tavily" },
  { id: "grok", label: "Grok" },
  { id: "brave", label: "Brave" },
];

const SETTINGS_BUTTON_CLASS = "text-xs";
const SETTINGS_INPUT_CLASS = "h-8 text-xs";
const SETTINGS_SELECT_TRIGGER_CLASS = "h-8 text-xs";

const SEARCH_CONTEXT_SIZE_OPTIONS: Array<{
  value: SearchContextSize;
  label: string;
}> = [
  { value: "low", label: "Low" },
  { value: "medium", label: "Medium" },
  { value: "high", label: "High" },
];

export function SettingsDialog({
  open,
  onOpenChange,
  onServicesRestarted,
  initialTarget,
}: SettingsDialogProps) {
  const { locale, t } = useI18n();
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
  const [defaultWorkspace, setDefaultWorkspace] = useState("");
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
  const [toolchainMode, setToolchainMode] = useState<ToolchainMode>("system");
  const [retry429Enabled, setRetry429Enabled] = useState(true);
  const [retry429MaxRetries, setRetry429MaxRetries] = useState("10");
  const [retry429Unlimited, setRetry429Unlimited] = useState(false);
  const [retry429DelaySeconds, setRetry429DelaySeconds] = useState("10");
  const [replaceProviderWebSearch, setReplaceProviderWebSearch] =
    useState(false);
  const [searchMaxResults, setSearchMaxResults] = useState(
    DEFAULT_SEARCH_MAX_RESULTS,
  );
  const [searchContextSize, setSearchContextSize] =
    useState<SearchContextSize>(DEFAULT_SEARCH_CONTEXT_SIZE);
  const [searchSources, setSearchSources] = useState<
    Record<SearchSourceId, SearchSourceForm>
  >(() => defaultSearchSourceForms());
  const [testingSearchSource, setTestingSearchSource] =
    useState<SearchSourceId | null>(null);
  const [testSearchResultsBySource, setTestSearchResultsBySource] = useState<
    Partial<Record<SearchSourceId, TestSearchResponse>>
  >({});
  const [testSearchErrorsBySource, setTestSearchErrorsBySource] = useState<
    Partial<Record<SearchSourceId, string>>
  >({});
  const [testSearchPopupSource, setTestSearchPopupSource] =
    useState<SearchSourceId | null>(null);
  const [ngrokToken, setNgrokToken] = useState("");
  const [ngrokDomain, setNgrokDomain] = useState("");
  const [cfToken, setCfToken] = useState("");
  const [cfHostname, setCfHostname] = useState("");
  const [installingPlugins, setInstallingPlugins] = useState<Set<string>>(
    () => new Set(),
  );
  const [managedPlugins, setManagedPlugins] = useState<ManagedPluginSummary[]>(
    [],
  );
  const [checkingPluginUpdates, setCheckingPluginUpdates] = useState(false);
  const [pluginUpdatesChecked, setPluginUpdatesChecked] = useState(false);
  const [installingManagedPlugins, setInstallingManagedPlugins] = useState<
    Set<string>
  >(() => new Set());
  const [loading, setLoading] = useState(true);
  const [settingsLoaded, setSettingsLoaded] = useState(false);
  const [saving, setSaving] = useState<SaveState>("idle");
  const [notice, setNotice] = useState<Notice | null>(null);
  const [authToken, setAuthToken] = useState("");
  const [settingsTab, setSettingsTab] = useState("general");
  const [focusedImPluginId, setFocusedImPluginId] = useState<string | null>(null);
  const pluginUpdatesAutoCheckedRef = useRef(false);
  const apiBridgeRetryForm = useMemo<ApiBridgeRetryFormState>(
    () => ({
      retry429Enabled,
      retry429MaxRetries,
      retry429Unlimited,
      retry429DelaySeconds,
    }),
    [
      retry429Enabled,
      retry429MaxRetries,
      retry429Unlimited,
      retry429DelaySeconds,
    ],
  );
  const apiBridgeRetryFormKey = useMemo(
    () =>
      isApiBridgeRetryFormReady(apiBridgeRetryForm)
        ? serializeApiBridgeRetryForm(apiBridgeRetryForm)
        : null,
    [apiBridgeRetryForm],
  );

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

  const hydrateApiBridge = useCallback((loadedSettings: AppSettings) => {
    const apiBridge = isRecord(loadedSettings.api_bridge)
      ? loadedSettings.api_bridge
      : {};
    const retry429 = isRecord(apiBridge.retry_429)
      ? apiBridge.retry_429
      : {};
    const maxRetries = retry429.max_retries;
    const delaySeconds = retry429.delay_seconds;
    const replaceWebSearch =
      apiBridge.replace_provider_web_search ?? apiBridge.replaceProviderWebSearch;

    const nextForm = {
      retry429Enabled:
        typeof retry429.enabled === "boolean" ? retry429.enabled : true,
      retry429Unlimited: maxRetries === null,
      retry429MaxRetries:
        typeof maxRetries === "number" && Number.isFinite(maxRetries)
          ? String(Math.max(0, Math.floor(maxRetries)))
          : "10",
      retry429DelaySeconds:
        typeof delaySeconds === "number" && Number.isFinite(delaySeconds)
          ? String(Math.max(1, Math.floor(delaySeconds)))
          : "10",
    };

    setRetry429Enabled(nextForm.retry429Enabled);
    setRetry429Unlimited(nextForm.retry429Unlimited);
    setRetry429MaxRetries(nextForm.retry429MaxRetries);
    setRetry429DelaySeconds(nextForm.retry429DelaySeconds);
    setReplaceProviderWebSearch(
      typeof replaceWebSearch === "boolean" ? replaceWebSearch : false,
    );
  }, []);

  const hydrateSearchTool = useCallback((loadedSettings: AppSettings) => {
    const searchTool = isRecord(loadedSettings.search_tool)
      ? loadedSettings.search_tool
      : {};
    const sources = isRecord(searchTool.sources) ? searchTool.sources : {};
    setSearchMaxResults(
      searchMaxResultsInput(searchTool.max_results ?? searchTool.maxResults),
    );
    setSearchContextSize(
      searchContextSizeValue(
        searchTool.search_context_size ?? searchTool.searchContextSize,
      ),
    );
    setSearchSources(() => {
      const next = defaultSearchSourceForms();
      for (const def of SEARCH_SOURCE_DEFS) {
        const source = isRecord(sources[def.id]) ? sources[def.id] : {};
        next[def.id] = {
          enabled: typeof source.enabled === "boolean" ? source.enabled : false,
          apiKey: typeof source.api_key === "string" ? source.api_key : "",
        };
      }
      return next;
    });
  }, []);

  const hydrateIntegrations = useCallback((loadedSettings: AppSettings) => {
    const integrations = loadedSettings.integrations;
    setMcpAutoInstall(integrations?.mcp_auto_install ?? true);
    setSkillAutoInstall(integrations?.skill_auto_install ?? true);
  }, []);

  const hydrateGeneral = useCallback((
    loadedSettings: AppSettings,
    effectiveDefaultWorkspace: string,
  ) => {
    setDefaultWorkspace(
      typeof loadedSettings.default_workspace === "string" &&
        loadedSettings.default_workspace.trim()
        ? loadedSettings.default_workspace.trim()
        : effectiveDefaultWorkspace,
    );
    setToolchainMode(
      loadedSettings.startkit?.toolchain_mode === "managed"
        ? "managed"
        : "system",
    );
  }, []);

  const readAuthToken = useCallback(async () => {
    try {
      const file = await invoke<AuthFile | null>("get_auth_token");
      const token = file?.token ?? "";
      setAuthToken(token);
      return token;
    } catch (error) {
      console.warn("[desktop-ui] get_auth_token failed:", error);
      setAuthToken("");
      return "";
    }
  }, []);

  const load = useCallback(async () => {
    setLoading(true);
    setSettingsLoaded(false);
    setManagedPlugins([]);
    setPluginUpdatesChecked(false);
    pluginUpdatesAutoCheckedRef.current = false;
    setAuthToken("");
    setNotice(null);
    try {
      const [
        loadedSettings,
        agentDefs,
        registry,
        discovered,
        tunnelDefs,
        managedPluginDefs,
        workspaceResponse,
      ] =
        await Promise.all([
          invoke<AppSettings>("get_settings"),
          invoke<AgentSummary[]>("list_agents"),
          invoke<PluginRegistryEntry[]>("list_plugin_registry"),
          invoke<DiscoveredChannelPlugin[]>("list_channel_plugins"),
          invoke<TunnelSummary[]>("list_tunnels"),
          invoke<ManagedPluginSummary[]>("list_managed_plugins"),
          apiFetch("/api/workspaces").then(async (response) => {
            if (!response.ok) throw new Error(`HTTP ${response.status}`);
            return WorkspacesResponseSchema.parse(await response.json());
          }),
        ]);
      const orderedAgents = orderAgents(agentDefs);
      setSettings(loadedSettings);
      setAgents(orderedAgents);
      setPluginRegistry(registry);
      setDiscoveredPlugins(discovered);
      setManagedPlugins(managedPluginDefs);
      setTunnels(tunnelDefs);
      hydrateAgents(loadedSettings, orderedAgents);
      hydrateChannels(loadedSettings, registry, discovered);
      hydrateTunnel(loadedSettings);
      hydrateProxy(loadedSettings);
      hydrateApiBridge(loadedSettings);
      hydrateSearchTool(loadedSettings);
      hydrateIntegrations(loadedSettings);
      hydrateGeneral(loadedSettings, workspaceResponse.default_workspace);
      await readAuthToken();
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
    hydrateApiBridge,
    hydrateChannels,
    hydrateGeneral,
    hydrateIntegrations,
    hydrateProxy,
    hydrateSearchTool,
    hydrateTunnel,
    readAuthToken,
  ]);

  useEffect(() => {
    if (open) void load();
  }, [open, load]);

  const changeSettingsTab = useCallback((value: string) => {
    setSettingsTab(value);
    setNotice(null);
  }, []);

  useEffect(() => {
    if (!open || !initialTarget) return;
    changeSettingsTab(initialTarget.tab);
    setFocusedImPluginId(
      initialTarget.tab === "im" ? initialTarget.pluginId ?? null : null,
    );
  }, [
    changeSettingsTab,
    initialTarget?.nonce,
    initialTarget?.pluginId,
    initialTarget?.tab,
    open,
  ]);

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

  const updateSearchSource = useCallback(
    (sourceId: SearchSourceId, patch: Partial<SearchSourceForm>) => {
      setSearchSources((previous) => ({
        ...previous,
        [sourceId]: {
          ...previous[sourceId],
          ...patch,
        },
      }));
      setTestSearchResultsBySource((previous) =>
        withoutSearchSourceKey(previous, sourceId),
      );
      setTestSearchErrorsBySource((previous) =>
        withoutSearchSourceKey(previous, sourceId),
      );
      setTestSearchPopupSource((previous) =>
        previous === sourceId ? null : previous,
      );
    },
    [],
  );

  const refreshPluginInventory = useCallback(async () => {
    setCheckingPluginUpdates(true);
    setPluginUpdatesChecked(true);
    setNotice(null);
    try {
      const plugins = await invoke<ManagedPluginSummary[]>(
        "refresh_managed_plugins",
      );
      setManagedPlugins(plugins);
    } catch (error) {
      setNotice({
        variant: "error",
        message: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setCheckingPluginUpdates(false);
    }
  }, []);

  const installManagedPlugin = useCallback(
    async (
      category: ManagedPluginCategory,
      id: string,
      successMessage = "Plugin refreshed.",
    ) => {
      const key = managedPluginKey(category, id);
      setInstallingManagedPlugins((prev) => new Set(prev).add(key));
      setNotice(null);
      try {
        const plugin = await invoke<ManagedPluginSummary>(
          "install_managed_plugin",
          {
            request: { category, id },
          },
        );
        setManagedPlugins((previous) =>
          mergeManagedPlugins(previous, [plugin]),
        );
        if (category === "im") {
          const discovered = await invoke<DiscoveredChannelPlugin[]>(
            "list_channel_plugins",
          );
          setDiscoveredPlugins(discovered);
        }
        setNotice({ variant: "success", message: successMessage });
      } catch (error) {
        setNotice({
          variant: "error",
          message: error instanceof Error ? error.message : String(error),
        });
      } finally {
        setInstallingManagedPlugins((prev) => {
          const next = new Set(prev);
          next.delete(key);
          return next;
        });
      }
    },
    [],
  );

  const testWebSearchSource = useCallback(async (sourceId: SearchSourceId) => {
    const source = searchSources[sourceId];
    if (!source?.apiKey.trim()) {
      setTestSearchErrorsBySource((previous) => ({
        ...previous,
        [sourceId]: "API key is required.",
      }));
      return;
    }
    const query = searchTestQueryForLocale(locale);
    setTestingSearchSource(sourceId);
    setNotice(null);
    setTestSearchErrorsBySource((previous) =>
      withoutSearchSourceKey(previous, sourceId),
    );
    setTestSearchResultsBySource((previous) =>
      withoutSearchSourceKey(previous, sourceId),
    );
    try {
      const response = await invoke<TestSearchResponse>("test_web_search", {
        request: {
          query,
          maxResults: normalizedSearchMaxResults(searchMaxResults),
          searchContextSize,
          sources: {
            [sourceId]: {
              enabled: true,
              apiKey: source.apiKey.trim(),
            },
          },
        },
      });
      setTestSearchResultsBySource((previous) => ({
        ...previous,
        [sourceId]: response,
      }));
      setNotice({
        variant: "success",
        message: "Search source test completed.",
      });
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setTestSearchErrorsBySource((previous) => ({
        ...previous,
        [sourceId]: message,
      }));
    } finally {
      setTestingSearchSource(null);
    }
  }, [locale, searchContextSize, searchMaxResults, searchSources]);

  const installPlugin = useCallback(
    async (pluginId: string, _githubUrl: string) => {
      setInstallingPlugins((prev) => new Set(prev).add(pluginId));
      try {
        await installManagedPlugin("im", pluginId);
      } finally {
        setInstallingPlugins((prev) => {
          const next = new Set(prev);
          next.delete(pluginId);
          return next;
        });
      }
    },
    [installManagedPlugin],
  );

  useEffect(() => {
    if (
      !open ||
      settingsTab !== "plugins" ||
      loading ||
      pluginUpdatesAutoCheckedRef.current
    ) {
      return;
    }
    pluginUpdatesAutoCheckedRef.current = true;
    void refreshPluginInventory();
  }, [
    loading,
    open,
    refreshPluginInventory,
    settingsTab,
  ]);

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
      await readAuthToken();
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
  }, [onServicesRestarted, readAuthToken]);

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

  const applyApiBridgeSettings = useCallback(async () => {
    if (!apiBridgeRetryFormKey) return;
    setSaving("api-bridge");
    setNotice(null);
    try {
      const nextSettings = buildApiBridgeSettings({
        settings,
        retry429Form: apiBridgeRetryForm,
      });
      await invoke("save_settings", { settings: nextSettings });
      setSettings(nextSettings);
      await invoke("restart_services");
      await readAuthToken();
      onServicesRestarted?.();
      setNotice({ variant: "success", message: "API bridge settings applied." });
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
    apiBridgeRetryForm,
    apiBridgeRetryFormKey,
    onServicesRestarted,
    readAuthToken,
  ]);

  const applyWebSearchSettings = useCallback(async () => {
    setSaving("web-search");
    setNotice(null);
    try {
      const nextSettings = buildWebSearchSettings({
        settings,
        replaceProviderWebSearch,
        maxResults: searchMaxResults,
        searchContextSize,
        sources: searchSources,
      });
      await invoke("save_settings", { settings: nextSettings });
      setSettings(nextSettings);
      await invoke("restart_services");
      await readAuthToken();
      onServicesRestarted?.();
      setNotice({ variant: "success", message: "Web search settings applied." });
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
    replaceProviderWebSearch,
    searchContextSize,
    searchMaxResults,
    searchSources,
    onServicesRestarted,
    readAuthToken,
  ]);

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
      setNotice({ variant: "success", message: "IM Channel settings applied." });
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

  const saveDefaultWorkspace = useCallback(async (workspacePath: string) => {
    const workspace = workspacePath.trim();
    if (!workspace) return;
    setSaving("general");
    setNotice(null);
    try {
      const nextSettings = buildGeneralSettings({
        settings,
        defaultWorkspace: workspace,
        toolchainMode,
      });
      await invoke("save_settings", { settings: nextSettings });
      setSettings(nextSettings);
      const response = await apiFetch("/api/settings/reload", { method: "POST" });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      setDefaultWorkspace(workspace);
      onServicesRestarted?.();
      setNotice({ variant: "success", message: "General settings applied." });
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
    toolchainMode,
    onServicesRestarted,
  ]);

  const saveToolchainMode = useCallback(async (mode: ToolchainMode) => {
    setToolchainMode(mode);
    setSaving("general");
    setNotice(null);
    try {
      const nextSettings = buildGeneralSettings({
        settings,
        defaultWorkspace,
        toolchainMode: mode,
      });
      await invoke("save_settings", { settings: nextSettings });
      setSettings(nextSettings);
      const response = await apiFetch("/api/settings/reload", { method: "POST" });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      onServicesRestarted?.();
      setNotice({ variant: "success", message: "General settings applied." });
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
    defaultWorkspace,
    onServicesRestarted,
  ]);

  const chooseDefaultWorkspace = useCallback(async () => {
    const selected = await openDirectoryDialog({
      directory: true,
      multiple: false,
      title: t("Choose Default Workspace"),
    });
    if (!selected) return;
    const path = typeof selected === "string" ? selected : selected[0];
    if (path) await saveDefaultWorkspace(path);
  }, [saveDefaultWorkspace, t]);

  const copyDefaultWorkspace = useCallback(async () => {
    if (!defaultWorkspace) return;
    try {
      await navigator.clipboard.writeText(defaultWorkspace);
      setNotice({ variant: "success", message: "Copied" });
    } catch (error) {
      setNotice({
        variant: "error",
        message: error instanceof Error ? error.message : String(error),
      });
    }
  }, [defaultWorkspace]);

  const copyAuthToken = useCallback(async () => {
    if (!authToken) return;
    try {
      await navigator.clipboard.writeText(authToken);
      setNotice({ variant: "success", message: "Copied" });
    } catch (error) {
      setNotice({
        variant: "error",
        message: error instanceof Error ? error.message : String(error),
      });
    }
  }, [authToken]);

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
          await readAuthToken();
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
      readAuthToken,
    ],
  );

  return (
    <>
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="!flex h-[680px] min-h-[520px] w-[min(860px,calc(100vw-32px))] max-h-[calc(100vh-64px)] max-w-[calc(100vw-32px)] overflow-hidden p-0 sm:max-w-[min(860px,calc(100vw-32px))]">
        <Tabs
          orientation="vertical"
          value={settingsTab}
          onValueChange={changeSettingsTab}
          className="min-h-0 flex-1 gap-0"
        >
          <aside className="flex w-44 shrink-0 flex-col border-r border-border bg-muted/20 px-4 py-4">
            <DialogHeader className="mb-4 pr-8">
              <DialogTitle className="text-base">{t("Settings")}</DialogTitle>
            </DialogHeader>
            <TabsList className="!h-auto w-full flex-col items-stretch justify-start gap-1 rounded-none bg-transparent p-0">
              <TabsTrigger
                value="general"
                className="!h-8 w-full justify-start gap-2 px-2 text-sm data-[state=active]:border-transparent data-[state=active]:bg-primary/10 data-[state=active]:text-primary data-[state=active]:shadow-none [&_svg:not([class*='size-'])]:!size-3.5"
              >
                <SlidersHorizontal className="h-3 w-3" />
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
                value="api-bridge"
                className="!h-8 w-full justify-start gap-2 px-2 text-sm data-[state=active]:border-transparent data-[state=active]:bg-primary/10 data-[state=active]:text-primary data-[state=active]:shadow-none [&_svg:not([class*='size-'])]:!size-3.5"
              >
                <RotateCw className="h-3 w-3" />
                {t("API Bridge")}
              </TabsTrigger>
              <TabsTrigger
                value="web-search"
                className="!h-8 w-full justify-start gap-2 px-2 text-sm data-[state=active]:border-transparent data-[state=active]:bg-primary/10 data-[state=active]:text-primary data-[state=active]:shadow-none [&_svg:not([class*='size-'])]:!size-3.5"
              >
                <Search className="h-3 w-3" />
                {t("Web Search")}
              </TabsTrigger>
              <TabsTrigger
                value="plugins"
                className="!h-8 w-full justify-start gap-2 px-2 text-sm data-[state=active]:border-transparent data-[state=active]:bg-primary/10 data-[state=active]:text-primary data-[state=active]:shadow-none [&_svg:not([class*='size-'])]:!size-3.5"
              >
                <Puzzle className="h-3 w-3" />
                {t("Plugins")}
              </TabsTrigger>
              <TabsTrigger
                value="im"
                className="!h-8 w-full justify-start gap-2 px-2 text-sm data-[state=active]:border-transparent data-[state=active]:bg-primary/10 data-[state=active]:text-primary data-[state=active]:shadow-none [&_svg:not([class*='size-'])]:!size-3.5"
              >
                <MessageSquare className="h-3 w-3" />
                {t("IM Channel")}
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
                    <SlidersHorizontal className="h-4 w-4 text-primary" />
                    {t("General")}
                  </h2>
                  <p className="mt-1 text-xs text-muted-foreground">
                    {t("Manage local service controls and rerun setup when needed.")}
                  </p>
                  <SettingsNotice notice={notice} />
                </div>
                <div className="rounded-md border border-border">
                  <div className="space-y-3 border-b border-border px-4 py-4">
                    <div className="flex items-start justify-between gap-4">
                      <div className="min-w-0 flex-1">
                        <div className="text-sm font-medium">
                          {t("Default System Workspace")}
                        </div>
                        <div className="mt-0.5 text-xs text-muted-foreground">
                          {t("New launch and IM workspaces are created under this folder.")}
                        </div>
                      </div>
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        className={`self-end ${SETTINGS_BUTTON_CLASS}`}
                        disabled={!canSubmit}
                        onClick={() => void chooseDefaultWorkspace()}
                      >
                        {saving === "general" ? (
                          <Loader2 className="h-3 w-3 animate-spin" />
                        ) : (
                          <FolderOpen className="h-3 w-3" />
                        )}
                        {saving === "general" ? t("Saving…") : t("Choose")}
                      </Button>
                    </div>
                    <div className="flex w-full items-center gap-2 rounded-md border border-border/70 bg-muted/20 px-2.5 py-1 shadow-xs">
                      <div
                        className="min-w-0 flex-1 overflow-x-auto whitespace-nowrap text-left text-xs leading-5 text-foreground"
                        title={defaultWorkspace}
                      >
                        {defaultWorkspace}
                      </div>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon-xs"
                        className="h-5 w-5 text-muted-foreground hover:text-foreground"
                        disabled={!defaultWorkspace}
                        aria-label={t("Copy")}
                        title={t("Copy")}
                        onClick={() => void copyDefaultWorkspace()}
                      >
                        <Copy className="h-3 w-3" />
                      </Button>
                    </div>
                  </div>
                  <SettingsActionRow
                    label={t("Agent Toolchain")}
                    description={t("Choose where VibeAround looks for agent CLIs when no manual executable path is set.")}
                    action={
                      <Select
                        value={toolchainMode}
                        onValueChange={(value) =>
                          void saveToolchainMode(value as ToolchainMode)
                        }
                        disabled={saving !== "idle"}
                      >
                        <SelectTrigger className="h-8 w-48 text-xs">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="system">{t("System")}</SelectItem>
                          <SelectItem value="managed">
                            {t("VibeAround managed")}
                          </SelectItem>
                        </SelectContent>
                      </Select>
                    }
                  />
                  <div className="space-y-3 border-b border-border px-4 py-4">
                    <div className="min-w-0">
                      <div className="text-sm font-medium">{t("Auth Token")}</div>
                      <div className="mt-0.5 text-xs text-muted-foreground">
                        {t("Current local service token from auth.json. Regenerated when services restart.")}
                      </div>
                    </div>
                    <div className="flex w-full items-center gap-2 rounded-md border border-border/70 bg-muted/20 px-2.5 py-1 shadow-xs">
                      <div
                        className={`min-w-0 flex-1 overflow-x-auto whitespace-nowrap text-left font-mono text-[11px] leading-5 ${
                          authToken ? "text-foreground" : "text-muted-foreground"
                        }`}
                        title={authToken || t("Unavailable")}
                      >
                        {authToken || (loading ? t("Loading…") : t("Unavailable"))}
                      </div>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon-xs"
                        className="h-5 w-5 text-muted-foreground hover:text-foreground"
                        disabled={!authToken || saving !== "idle"}
                        aria-label={t("Copy")}
                        title={t("Copy")}
                        onClick={() => void copyAuthToken()}
                      >
                        <Copy className="h-3 w-3" />
                      </Button>
                    </div>
                  </div>
                  <SettingsActionRow
                    label={t("Restart Services")}
                    description={t("Restart VibeAround runtime services after local changes.")}
                    action={
                      <Button
                        type="button"
                        size="sm"
                        className={SETTINGS_BUTTON_CLASS}
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
                        className={SETTINGS_BUTTON_CLASS}
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
                      compact
                      focusPluginId={focusedImPluginId}
                      notice={<SettingsNotice notice={notice} />}
                    />
                  </div>
                  <div className="flex shrink-0 justify-end border-t border-border px-5 py-3">
                    <Button
                      type="button"
                      size="sm"
                      className={SETTINGS_BUTTON_CLASS}
                      disabled={!canSubmit}
                      onClick={() => void applyImSettings()}
                    >
                      {saving === "im"
                        ? t("Applying…")
                        : t("Apply IM Channel Settings")}
                    </Button>
                  </div>
                </>
              )}
            </TabsContent>

            <TabsContent
              value="plugins"
              className="min-h-0 overflow-hidden data-[state=active]:flex data-[state=active]:flex-col"
            >
              {loading ? (
                <div className="min-h-0 flex-1 overflow-y-auto px-5 py-5 [scrollbar-gutter:stable]">
                  <LoadingBlock />
                </div>
              ) : (
                <div className="min-h-0 flex-1 overflow-y-auto px-5 py-5 [scrollbar-gutter:stable]">
                  <PluginsSettingsPanel
                    plugins={managedPlugins}
                    installingPlugins={installingManagedPlugins}
                    checkingUpdates={checkingPluginUpdates}
                    updatesChecked={pluginUpdatesChecked}
                    onInstallPlugin={installManagedPlugin}
                    onConfigureSearch={() => changeSettingsTab("web-search")}
                    onCheckUpdates={() => void refreshPluginInventory()}
                    notice={<SettingsNotice notice={notice} />}
                  />
                </div>
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
                      className={SETTINGS_BUTTON_CLASS}
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
              value="api-bridge"
              className="min-h-0 overflow-hidden data-[state=active]:flex data-[state=active]:flex-col"
            >
              {loading ? (
                <div className="min-h-0 flex-1 overflow-y-auto px-5 py-5 [scrollbar-gutter:stable]">
                  <LoadingBlock />
                </div>
              ) : (
                <>
                  <div className="min-h-0 flex-1 overflow-y-auto px-5 py-5 [scrollbar-gutter:stable]">
                    <div className="space-y-6">
                      <ApiBridgeRetrySettingsPanel
                        retry429Enabled={retry429Enabled}
                        retry429MaxRetries={retry429MaxRetries}
                        retry429Unlimited={retry429Unlimited}
                        retry429DelaySeconds={retry429DelaySeconds}
                        onRetry429EnabledChange={setRetry429Enabled}
                        onRetry429MaxRetriesChange={setRetry429MaxRetries}
                        onRetry429UnlimitedChange={setRetry429Unlimited}
                        onRetry429DelaySecondsChange={setRetry429DelaySeconds}
                        disabled={!canSubmit}
                        notice={<SettingsNotice notice={notice} />}
                      />
                    </div>
                  </div>
                  <div className="flex shrink-0 justify-end border-t border-border px-5 py-3">
                    <Button
                      type="button"
                      size="sm"
                      className={SETTINGS_BUTTON_CLASS}
                      disabled={!canSubmit || !apiBridgeRetryFormKey}
                      onClick={() => void applyApiBridgeSettings()}
                    >
                      {saving === "api-bridge"
                        ? t("Restarting services…")
                        : t("Apply & Restart Services")}
                    </Button>
                  </div>
                </>
              )}
            </TabsContent>

            <TabsContent
              value="web-search"
              className="min-h-0 overflow-hidden data-[state=active]:flex data-[state=active]:flex-col"
            >
              {loading ? (
                <div className="min-h-0 flex-1 overflow-y-auto px-5 py-5 [scrollbar-gutter:stable]">
                  <LoadingBlock />
                </div>
              ) : (
                <>
                  <div className="min-h-0 flex-1 overflow-y-auto px-5 py-5 [scrollbar-gutter:stable]">
                    <SearchToolSettingsPanel
                      replaceProviderWebSearch={replaceProviderWebSearch}
                      maxResults={searchMaxResults}
                      searchContextSize={searchContextSize}
                      sources={searchSources}
                      testResults={testSearchResultsBySource}
                      testErrors={testSearchErrorsBySource}
                      testingSource={testingSearchSource}
                      onReplaceProviderWebSearchChange={
                        setReplaceProviderWebSearch
                      }
                      onMaxResultsChange={setSearchMaxResults}
                      onSearchContextSizeChange={setSearchContextSize}
                      onSourceChange={updateSearchSource}
                      onTestSource={(sourceId) =>
                        void testWebSearchSource(sourceId)
                      }
                      onOpenTestResult={setTestSearchPopupSource}
                      notice={<SettingsNotice notice={notice} />}
                    />
                  </div>
                  <div className="flex shrink-0 justify-end border-t border-border px-5 py-3">
                    <Button
                      type="button"
                      size="sm"
                      className={SETTINGS_BUTTON_CLASS}
                      disabled={!canSubmit}
                      onClick={() => void applyWebSearchSettings()}
                    >
                      {saving === "web-search"
                        ? t("Restarting services…")
                        : t("Apply & Restart Services")}
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
                      className={SETTINGS_BUTTON_CLASS}
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
                      showProviderSelect
                      compact
                      notice={<SettingsNotice notice={notice} />}
                    />
                  </div>
                  <div className="flex shrink-0 flex-wrap justify-end gap-2 border-t border-border px-5 py-3">
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      className={SETTINGS_BUTTON_CLASS}
                      disabled={!canSubmit}
                      onClick={() => void saveTunnelSettings(false)}
                    >
                      {saving === "tunnel" ? t("Saving…") : t("Save")}
                    </Button>
                    <Button
                      type="button"
                      size="sm"
                      className={SETTINGS_BUTTON_CLASS}
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
    <SearchSourceTestResultDialog
      open={testSearchPopupSource !== null}
      sourceId={testSearchPopupSource}
      result={
        testSearchPopupSource
          ? testSearchResultsBySource[testSearchPopupSource] ?? null
          : null
      }
      onOpenChange={(nextOpen) => {
        if (!nextOpen) setTestSearchPopupSource(null);
      }}
    />
    </>
  );
}

type PluginInstallStatusFilter =
  | "all"
  | "updates"
  | "installed"
  | "not-installed";
type PluginCategoryFilter = "all" | "acp" | "im" | "search";

type PluginInventoryItem = ManagedPluginSummary;

function PluginsSettingsPanel({
  plugins,
  installingPlugins,
  checkingUpdates,
  updatesChecked,
  onInstallPlugin,
  onConfigureSearch,
  onCheckUpdates,
  notice,
}: {
  plugins: ManagedPluginSummary[];
  installingPlugins: Set<string>;
  checkingUpdates: boolean;
  updatesChecked: boolean;
  onInstallPlugin: (category: ManagedPluginCategory, id: string) => void;
  onConfigureSearch: () => void;
  onCheckUpdates: () => void;
  notice?: ReactNode;
}) {
  const { t } = useI18n();
  const [statusFilter, setStatusFilter] =
    useState<PluginInstallStatusFilter>("all");
  const [categoryFilter, setCategoryFilter] =
    useState<PluginCategoryFilter>("all");

  const items = plugins;

  const updateItems = items.filter((item) => item.status === "outdated");
  const installedItems = items.filter((item) => item.installed);
  const notInstalledItems = items.filter((item) => !item.installed);
  const visibleItems = items.filter((item) => {
    const statusMatch =
      statusFilter === "all" ||
      (statusFilter === "updates" && item.status === "outdated") ||
      (statusFilter === "installed" && item.installed) ||
      (statusFilter === "not-installed" && !item.installed);
    const categoryMatch =
      categoryFilter === "all" || item.category === categoryFilter;
    return statusMatch && categoryMatch;
  });

  const statusFilterOptions: Array<{
    value: PluginInstallStatusFilter;
    label: string;
    count: number;
  }> = [
    { value: "all", label: "All", count: items.length },
    { value: "updates", label: "Needs update", count: updateItems.length },
    { value: "installed", label: "Installed", count: installedItems.length },
    {
      value: "not-installed",
      label: "Not installed",
      count: notInstalledItems.length,
    },
  ];
  const allCategoryFilterOptions: Array<{
    value: PluginCategoryFilter;
    label: string;
    count: number;
  }> = [
    { value: "all", label: "All categories", count: items.length },
    {
      value: "acp",
      label: "ACP plugin",
      count: items.filter((item) => item.category === "acp").length,
    },
    {
      value: "im",
      label: "IM plugin",
      count: items.filter((item) => item.category === "im").length,
    },
    {
      value: "search",
      label: "Search plugin",
      count: items.filter((item) => item.category === "search").length,
    },
  ];
  const categoryFilterOptions = allCategoryFilterOptions.filter(
    (option) => option.value === "all" || option.count > 0,
  );

  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="flex items-center gap-2 text-base font-semibold">
            <Puzzle className="h-4 w-4 text-primary" />
            {t("Plugins")}
          </h2>
          <p className="mt-1 text-xs text-muted-foreground">
            {t("Manage installed and installable plugins from one inventory.")}
          </p>
          {notice}
        </div>
        <Button
          type="button"
          variant="outline"
          size="sm"
          className={SETTINGS_BUTTON_CLASS}
          disabled={checkingUpdates}
          onClick={onCheckUpdates}
        >
          {checkingUpdates ? (
            <Loader2 className="h-3 w-3 animate-spin" />
          ) : (
            <RotateCw className="h-3 w-3" />
          )}
          {checkingUpdates ? t("Refreshing…") : t("Refresh status")}
        </Button>
      </div>

      <div className="space-y-3 rounded-md border border-border px-3 py-3">
        <PluginFilterGroup
          label={t("Install status")}
          options={statusFilterOptions}
          value={statusFilter}
          onChange={setStatusFilter}
        />
        <PluginFilterGroup
          label={t("Category")}
          options={categoryFilterOptions}
          value={categoryFilter}
          onChange={setCategoryFilter}
        />
      </div>

      {!updatesChecked && (
        <StatusBanner variant="warning">
          {t("Plugin status has not been refreshed yet.")}
        </StatusBanner>
      )}

      <div className="space-y-2">
        {visibleItems.length === 0 ? (
          <div className="rounded-md border border-dashed border-border px-4 py-8 text-center text-xs text-muted-foreground">
            {t("No plugins in this view.")}
          </div>
        ) : (
          visibleItems.map((item) => (
            <PluginInventoryCard
              key={managedPluginKey(item.category, item.id)}
              item={item}
              installing={installingPlugins.has(
                managedPluginKey(item.category, item.id),
              )}
              onInstallPlugin={onInstallPlugin}
              onConfigureSearch={onConfigureSearch}
            />
          ))
        )}
      </div>
    </div>
  );
}

function PluginFilterGroup<T extends string>({
  label,
  options,
  value,
  onChange,
}: {
  label: string;
  options: Array<{ value: T; label: string; count: number }>;
  value: T;
  onChange: (value: T) => void;
}) {
  const { t } = useI18n();
  return (
    <div className="grid gap-2 sm:grid-cols-[110px_1fr] sm:items-center">
      <div className="text-xs font-medium text-muted-foreground">{label}</div>
      <div className="flex flex-wrap gap-1.5">
        {options.map((option) => {
          const active = value === option.value;
          return (
            <button
              key={option.value}
              type="button"
              className={`flex h-7 items-center gap-1.5 rounded-md border px-2.5 text-xs transition-colors ${
                active
                  ? "border-primary/40 bg-primary/5 text-primary"
                  : "border-border text-muted-foreground hover:border-border/80 hover:text-foreground"
              }`}
              onClick={() => onChange(option.value)}
            >
              <span>{t(option.label)}</span>
              <span className="font-mono text-[10px] opacity-70">
                {option.count}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

function PluginInventoryCard({
  item,
  installing,
  onInstallPlugin,
  onConfigureSearch,
}: {
  item: PluginInventoryItem;
  installing: boolean;
  onInstallPlugin: (category: ManagedPluginCategory, id: string) => void;
  onConfigureSearch: () => void;
}) {
  const { t } = useI18n();
  const status = pluginStatus(item);
  const categoryLabel = pluginCategoryLabel(item.category);
  const showKindBadge = item.kind !== categoryLabel;
  const ActionIcon =
    item.status === "outdated"
        ? RotateCw
        : item.installed
          ? RotateCw
          : Download;
  const canRunAction = Boolean(item.installable);
  const actionLabel = pluginActionLabel(item);
  const Icon =
    item.category === "acp"
      ? Bot
      : item.category === "im"
        ? MessageSquare
        : Search;

  return (
    <section className="rounded-md border border-border bg-card px-4 py-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="flex min-w-0 flex-1 gap-3">
          <span className="mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-md border border-border/70 bg-muted/30 text-primary">
            <Icon className="h-4 w-4" />
          </span>
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2">
              <span className="text-sm font-medium">{item.name}</span>
              <Badge variant="outline" className="rounded-md text-[10px]">
                {t(categoryLabel)}
              </Badge>
              {showKindBadge && (
                <Badge variant="outline" className="rounded-md text-[10px]">
                  {item.kind}
                </Badge>
              )}
              {item.source && (
                <Badge variant="secondary" className="rounded-md text-[10px]">
                  {t(item.source === "project" ? "Project" : "User")}
                </Badge>
              )}
              <Badge
                variant="outline"
                className={`rounded-md text-[10px] ${status.className}`}
              >
                {t(status.label)}
              </Badge>
            </div>
            <p className="mt-1 text-xs text-muted-foreground">
              {t(item.description)}
            </p>
            <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-muted-foreground">
              <span className="font-mono">{item.id}</span>
              {item.version && (
                <span>
                  {t("Installed")} {item.version}
                </span>
              )}
              {item.latestVersion && item.latestVersion !== item.version && (
                <span>
                  {t("Latest")} {item.latestVersion}
                </span>
              )}
              {item.message && (
                <span className="text-muted-foreground/80">
                  {t(item.message)}
                </span>
              )}
            </div>
          </div>
        </div>

        <div className="flex shrink-0 items-center gap-2">
          {item.github && (
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              asChild
            >
              <a
                href={item.github}
                target="_blank"
                rel="noopener noreferrer"
                aria-label={t("View on GitHub")}
              >
                <ExternalLink className="h-3.5 w-3.5" />
              </a>
            </Button>
          )}
          {item.category === "search" && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              className={SETTINGS_BUTTON_CLASS}
              onClick={onConfigureSearch}
            >
              <SlidersHorizontal className="h-3 w-3" />
              {t("Configure")}
            </Button>
          )}
          <Button
            type="button"
            variant={item.status === "outdated" ? "default" : "outline"}
            size="sm"
            className={`min-w-20 ${SETTINGS_BUTTON_CLASS}`}
            disabled={installing || !canRunAction}
            onClick={() => onInstallPlugin(item.category, item.id)}
          >
            {installing ? (
              <Loader2 className="h-3 w-3 animate-spin" />
            ) : (
              <ActionIcon className="h-3 w-3" />
            )}
            {installing ? t("Installing…") : t(actionLabel)}
          </Button>
        </div>
      </div>
    </section>
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
              className={SETTINGS_BUTTON_CLASS}
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
              className={SETTINGS_BUTTON_CLASS}
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
          label={t("Enable HTTP proxy")}
          description={t("Allow profiles to opt in to this HTTP proxy.")}
          action={
            <Switch
              checked={proxyEnabled}
              onCheckedChange={onProxyEnabledChange}
              aria-label={t("Enable HTTP proxy")}
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
              className={`mt-1 ${SETTINGS_INPUT_CLASS}`}
            />
          </label>
        </div>
        <div className="grid gap-3 border-b border-border px-4 py-4 last:border-b-0">
          <label className="block">
            <span className="text-xs text-muted-foreground">
              {t("Proxy bypass list")}
            </span>
            <Input
              type="text"
              value={proxyNoProxy}
              onChange={(event) => onProxyNoProxyChange(event.currentTarget.value)}
              placeholder="localhost,127.0.0.1,::1"
              className={`mt-1 ${SETTINGS_INPUT_CLASS}`}
            />
            <span className="mt-1 block text-[11px] text-muted-foreground/70">
              {t("Comma-separated hosts, domains, or IPs that should connect directly.")}
            </span>
          </label>
        </div>
      </div>
    </div>
  );
}

function SearchToolSettingsPanel({
  replaceProviderWebSearch,
  maxResults,
  searchContextSize,
  sources,
  testResults,
  testErrors,
  testingSource,
  onReplaceProviderWebSearchChange,
  onMaxResultsChange,
  onSearchContextSizeChange,
  onSourceChange,
  onTestSource,
  onOpenTestResult,
  notice,
}: {
  replaceProviderWebSearch: boolean;
  maxResults: string;
  searchContextSize: SearchContextSize;
  sources: Record<SearchSourceId, SearchSourceForm>;
  testResults: Partial<Record<SearchSourceId, TestSearchResponse>>;
  testErrors: Partial<Record<SearchSourceId, string>>;
  testingSource: SearchSourceId | null;
  onReplaceProviderWebSearchChange: (value: boolean) => void;
  onMaxResultsChange: (value: string) => void;
  onSearchContextSizeChange: (value: SearchContextSize) => void;
  onSourceChange: (sourceId: SearchSourceId, patch: Partial<SearchSourceForm>) => void;
  onTestSource: (sourceId: SearchSourceId) => void;
  onOpenTestResult: (sourceId: SearchSourceId) => void;
  notice?: ReactNode;
}) {
  const { t } = useI18n();
  return (
    <div className="space-y-5">
      <div>
        <h2 className="flex items-center gap-2 text-base font-semibold">
          <Search className="h-4 w-4 text-primary" />
          {t("Web search")}
        </h2>
        <p className="mt-1 text-xs text-muted-foreground">
          {t("Host-side web search is available when at least one search source is enabled.")}
        </p>
        {notice}
      </div>
      <div className="rounded-md border border-border">
        <SettingsActionRow
          label={t("Replace provider web search")}
          description={t("Use VibeAround host search even when the upstream model supports provider-native web_search.")}
          action={
            <Switch
              checked={replaceProviderWebSearch}
              onCheckedChange={onReplaceProviderWebSearchChange}
              aria-label={t("Replace provider web search")}
              size="sm"
            />
          }
        />
      </div>
      <div className="rounded-md border border-border px-4 py-4">
        <div className="grid gap-4 sm:grid-cols-2">
          <label className="block">
            <span className="text-xs text-muted-foreground">
              {t("Max results per source")}
            </span>
            <Input
              type="number"
              min={SEARCH_MAX_RESULTS_MIN}
              max={SEARCH_MAX_RESULTS_MAX}
              step={1}
              value={maxResults}
              onChange={(event) =>
                onMaxResultsChange(event.currentTarget.value)
              }
              className={`mt-1 ${SETTINGS_INPUT_CLASS}`}
            />
            <span className="mt-1 block text-[11px] text-muted-foreground/70">
              {t("Applied to each enabled source, not the combined total.")}
            </span>
          </label>
          <label className="block">
            <span className="text-xs text-muted-foreground">
              {t("Search context size")}
            </span>
            <Select
              value={searchContextSize}
              onValueChange={(value) =>
                onSearchContextSizeChange(value as SearchContextSize)
              }
            >
              <SelectTrigger
                size="sm"
                className={`mt-1 w-full ${SETTINGS_SELECT_TRIGGER_CLASS}`}
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {SEARCH_CONTEXT_SIZE_OPTIONS.map((option) => (
                  <SelectItem key={option.value} value={option.value}>
                    {t(option.label)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <span className="mt-1 block text-[11px] text-muted-foreground/70">
              {t("Controls provider depth and how much result content is returned.")}
            </span>
          </label>
        </div>
      </div>
      <div className="rounded-md border border-border">
        {SEARCH_SOURCE_DEFS.map((source) => {
          const form = sources[source.id];
          const result = testResults[source.id];
          const error = testErrors[source.id];
          const isTesting = testingSource === source.id;
          const canTest =
            !testingSource && Boolean(form.apiKey.trim());
          return (
            <div
              key={source.id}
              className="space-y-3 border-b border-border px-4 py-4 last:border-b-0"
            >
              <div className="flex items-center justify-between gap-3">
                <div className="min-w-0">
                  <div className="text-sm font-medium">{source.label}</div>
                </div>
                <div className="flex shrink-0 items-center gap-2">
                  {result && (
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      className={SETTINGS_BUTTON_CLASS}
                      onClick={() => onOpenTestResult(source.id)}
                    >
                      <ExternalLink className="h-3 w-3" />
                      {t("View result")}
                    </Button>
                  )}
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    className={SETTINGS_BUTTON_CLASS}
                    disabled={!canTest}
                    onClick={() => onTestSource(source.id)}
                  >
                    {isTesting ? (
                      <Loader2 className="h-3 w-3 animate-spin" />
                    ) : (
                      <Search className="h-3 w-3" />
                    )}
                    {isTesting ? t("Testing…") : t("Test")}
                  </Button>
                  <Switch
                    checked={form.enabled}
                    onCheckedChange={(value) =>
                      onSourceChange(source.id, { enabled: value })
                    }
                    aria-label={t("Enable source")}
                    size="sm"
                  />
                </div>
              </div>
              <label className="block">
                <span className="text-xs text-muted-foreground">
                  {t("API key")}
                </span>
                <Input
                  type="password"
                  value={form.apiKey}
                  placeholder={t("Paste API key")}
                  onChange={(event) =>
                    onSourceChange(source.id, {
                      apiKey: event.currentTarget.value,
                    })
                  }
                  className={`mt-1 ${SETTINGS_INPUT_CLASS}`}
                />
              </label>
              {error && (
                <div className="rounded-md border border-destructive/25 bg-destructive/5 px-2 py-1.5 text-xs text-destructive">
                  <div className="flex items-center gap-1 font-medium">
                    <AlertCircle className="h-3 w-3" />
                    {t("Test failed")}
                  </div>
                  <div className="mt-0.5 break-words text-[11px]">
                    {error === "API key is required." ? t(error) : error}
                  </div>
                </div>
              )}
              {result && (
                <div className="flex flex-wrap items-center gap-2 text-[11px] text-muted-foreground">
                  <span className="inline-flex items-center gap-1 font-medium text-primary">
                    <CheckCircle2 className="h-3 w-3" />
                    {t("Test passed")}
                  </span>
                  <span className="font-mono text-foreground/70">
                    {result.query}
                  </span>
                  <span>
                    {t("{{count}} results", {
                      count: result.results.length,
                    })}
                  </span>
                  <span>
                    {t("{{count}} citations", {
                      count: result.citations.length,
                    })}
                  </span>
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function SearchSourceTestResultDialog({
  open,
  sourceId,
  result,
  onOpenChange,
}: {
  open: boolean;
  sourceId: SearchSourceId | null;
  result: TestSearchResponse | null;
  onOpenChange: (open: boolean) => void;
}) {
  const { t } = useI18n();
  const sourceLabel = sourceId ? searchSourceLabel(sourceId) : t("Search");
  const results = result?.results ?? [];
  const citations = result?.citations ?? [];

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="!flex h-[min(720px,calc(100vh-64px))] min-h-[420px] w-[min(760px,calc(100vw-48px))] max-w-[calc(100vw-48px)] flex-col overflow-hidden p-0 sm:max-w-[min(760px,calc(100vw-48px))]">
        <DialogHeader className="border-b border-border px-5 py-4">
          <DialogTitle className="flex items-center gap-2 text-base">
            <Search className="h-4 w-4 text-primary" />
            {t("{{source}} test result", { source: sourceLabel })}
          </DialogTitle>
          {result && (
            <div className="flex flex-wrap items-center gap-x-3 gap-y-1 pt-1 text-xs text-muted-foreground">
              <span className="font-mono text-foreground/80">
                {result.provider}
              </span>
              <span className="font-mono">{result.query}</span>
              <span>
                {t("{{count}} results", { count: results.length })}
              </span>
              <span>
                {t("{{count}} citations", { count: citations.length })}
              </span>
            </div>
          )}
        </DialogHeader>
        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4 [scrollbar-gutter:stable]">
          {!result ? (
            <div className="rounded-md border border-border bg-muted/15 px-3 py-6 text-center text-sm text-muted-foreground">
              {t("No test result yet.")}
            </div>
          ) : results.length === 0 ? (
            <div className="rounded-md border border-border bg-muted/15 px-3 py-6 text-center text-sm text-muted-foreground">
              {t("No results returned.")}
            </div>
          ) : (
            <div className="space-y-3">
              {results.map((item, index) => (
                <div
                  key={`${item.source}:${item.url}:${index}`}
                  className="rounded-md border border-border bg-background px-3 py-3"
                >
                  <div className="flex min-w-0 items-start justify-between gap-3">
                    <div className="min-w-0">
                      <div className="flex min-w-0 flex-wrap items-center gap-2">
                        <Badge variant="secondary" className="h-5 rounded-md px-1.5 font-mono text-[10px]">
                          {item.source}
                        </Badge>
                        {typeof item.score === "number" && (
                          <span className="font-mono text-[10px] text-muted-foreground">
                            {item.score.toFixed(3)}
                          </span>
                        )}
                        {item.publishedDate && (
                          <span className="text-[10px] text-muted-foreground">
                            {item.publishedDate}
                          </span>
                        )}
                      </div>
                      <div className="mt-2 break-words text-sm font-medium leading-5">
                        {item.title || item.url}
                      </div>
                    </div>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon-sm"
                      asChild
                    >
                      <a
                        href={item.url}
                        target="_blank"
                        rel="noreferrer"
                        aria-label={t("Open result")}
                      >
                        <ExternalLink className="h-3.5 w-3.5" />
                      </a>
                    </Button>
                  </div>
                  <div className="mt-2 break-all font-mono text-[11px] leading-5 text-muted-foreground">
                    {item.url}
                  </div>
                  {item.snippet && (
                    <div className="mt-2 text-xs leading-5 text-muted-foreground">
                      {item.snippet}
                    </div>
                  )}
                  {item.content && (
                    <div className="mt-2 max-h-40 overflow-y-auto rounded-md border border-border/70 bg-muted/20 px-2.5 py-2 text-xs leading-5 text-muted-foreground [scrollbar-gutter:stable]">
                      {item.content}
                    </div>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}

function ApiBridgeRetrySettingsPanel({
  retry429Enabled,
  retry429MaxRetries,
  retry429Unlimited,
  retry429DelaySeconds,
  onRetry429EnabledChange,
  onRetry429MaxRetriesChange,
  onRetry429UnlimitedChange,
  onRetry429DelaySecondsChange,
  disabled,
  notice,
}: {
  retry429Enabled: boolean;
  retry429MaxRetries: string;
  retry429Unlimited: boolean;
  retry429DelaySeconds: string;
  onRetry429EnabledChange: (value: boolean) => void;
  onRetry429MaxRetriesChange: (value: string) => void;
  onRetry429UnlimitedChange: (value: boolean) => void;
  onRetry429DelaySecondsChange: (value: string) => void;
  disabled: boolean;
  notice?: ReactNode;
}) {
  const { t } = useI18n();
  const controlsDisabled = disabled || !retry429Enabled;
  return (
    <div className="space-y-5">
      <div>
        <h2 className="flex items-center gap-2 text-base font-semibold">
          <RotateCw className="h-4 w-4 text-primary" />
          {t("API Bridge")}
        </h2>
        <p className="mt-1 text-xs text-muted-foreground">
          {t("Configure bridge-wide request behavior for provider API calls.")}
        </p>
        {notice}
      </div>
      <div className="rounded-md border border-border">
        <SettingsActionRow
          label={t("Auto retry 429")}
          description={t("Retry upstream API requests when the provider reports rate limiting.")}
          action={
            <Switch
              checked={retry429Enabled}
              onCheckedChange={onRetry429EnabledChange}
              aria-label={t("Auto retry 429")}
              size="sm"
              disabled={disabled}
            />
          }
        />
        <div className="grid gap-3 border-b border-border px-4 py-4 sm:grid-cols-[1fr_auto] sm:items-center">
          <div className="min-w-0">
            <div className="text-sm font-medium">{t("Max retries")}</div>
            <div className="mt-0.5 text-xs text-muted-foreground">
              {t("Set to unlimited to keep waiting through provider throttling.")}
            </div>
          </div>
          <div className="flex flex-wrap items-center justify-end gap-3">
            <Input
              type="number"
              min={0}
              step={1}
              value={retry429MaxRetries}
              onChange={(event) =>
                onRetry429MaxRetriesChange(event.currentTarget.value)
              }
              className={`w-24 text-right ${SETTINGS_INPUT_CLASS}`}
              disabled={controlsDisabled || retry429Unlimited}
              aria-label={t("Max retries")}
            />
            <label className="flex items-center gap-2 whitespace-nowrap text-xs text-muted-foreground">
              <Checkbox
                checked={retry429Unlimited}
                onCheckedChange={(checked) =>
                  onRetry429UnlimitedChange(checked === true)
                }
                disabled={controlsDisabled}
              />
              {t("Retry indefinitely")}
            </label>
          </div>
        </div>
        <div className="grid gap-3 px-4 py-4 sm:grid-cols-[1fr_auto] sm:items-center">
          <div className="min-w-0">
            <div className="text-sm font-medium">{t("Delay seconds")}</div>
            <div className="mt-0.5 text-xs text-muted-foreground">
              {t("Used between 429 retries unless upstream sends Retry-After.")}
            </div>
          </div>
          <Input
            type="number"
            min={1}
            step={1}
            value={retry429DelaySeconds}
            onChange={(event) =>
              onRetry429DelaySecondsChange(event.currentTarget.value)
            }
            className={`w-24 justify-self-end text-right ${SETTINGS_INPUT_CLASS}`}
            disabled={controlsDisabled}
            aria-label={t("Delay seconds")}
          />
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

function buildGeneralSettings({
  settings,
  defaultWorkspace,
  toolchainMode,
}: {
  settings: AppSettings;
  defaultWorkspace: string;
  toolchainMode: ToolchainMode;
}): AppSettings {
  const result: AppSettings = { ...settings };
  const workspace = defaultWorkspace.trim();
  if (workspace) {
    result.default_workspace = workspace;
  } else {
    delete result.default_workspace;
  }
  result.startkit = {
    ...(isRecord(settings.startkit) ? settings.startkit : {}),
    toolchain_mode: toolchainMode,
  };
  return result;
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

function buildApiBridgeSettings({
  settings,
  retry429Form,
}: {
  settings: AppSettings;
  retry429Form: ApiBridgeRetryFormState;
}): AppSettings {
  const result: AppSettings = { ...settings };
  const apiBridge = isRecord(settings.api_bridge)
    ? { ...settings.api_bridge }
    : {};
  const retry429 = isRecord(apiBridge.retry_429)
    ? { ...apiBridge.retry_429 }
    : {};

  retry429.enabled = retry429Form.retry429Enabled;
  retry429.max_retries = retry429Form.retry429Unlimited
    ? null
    : parseIntegerSetting(retry429Form.retry429MaxRetries, 10, 0);
  retry429.delay_seconds = parseIntegerSetting(
    retry429Form.retry429DelaySeconds,
    10,
    1,
  );

  apiBridge.retry_429 = retry429;
  result.api_bridge = apiBridge as AppSettings["api_bridge"];
  return result;
}

function buildWebSearchSettings({
  settings,
  replaceProviderWebSearch,
  maxResults,
  searchContextSize,
  sources,
}: {
  settings: AppSettings;
  replaceProviderWebSearch: boolean;
  maxResults: string;
  searchContextSize: SearchContextSize;
  sources: Record<SearchSourceId, SearchSourceForm>;
}): AppSettings {
  const result = buildSearchToolSettings({
    settings,
    maxResults,
    searchContextSize,
    sources,
  });
  const apiBridge = isRecord(result.api_bridge)
    ? { ...result.api_bridge }
    : {};
  apiBridge.replace_provider_web_search = replaceProviderWebSearch;
  delete apiBridge.replaceProviderWebSearch;
  result.api_bridge = apiBridge as AppSettings["api_bridge"];
  return result;
}

function buildSearchToolSettings({
  settings,
  maxResults,
  searchContextSize,
  sources,
}: {
  settings: AppSettings;
  maxResults: string;
  searchContextSize: SearchContextSize;
  sources: Record<SearchSourceId, SearchSourceForm>;
}): AppSettings {
  const result: AppSettings = { ...settings };
  const existingSearchTool = isRecord(settings.search_tool)
    ? settings.search_tool
    : {};
  const existingSources = isRecord(existingSearchTool.sources)
    ? existingSearchTool.sources
    : {};
  const nextSearchTool: Record<string, unknown> = {
    ...existingSearchTool,
    max_results: normalizedSearchMaxResults(maxResults),
    search_context_size: searchContextSize,
  };
  delete nextSearchTool.enabled;
  const nextSources: Record<string, unknown> = {};

  for (const [name, source] of Object.entries(existingSources)) {
    nextSources[name] = isRecord(source) ? { ...source } : source;
  }

  for (const def of SEARCH_SOURCE_DEFS) {
    const source = sources[def.id];
    const existingSource = nextSources[def.id];
    const payload: Record<string, unknown> = isRecord(existingSource)
      ? { ...existingSource }
      : {};
    payload.enabled = source.enabled;
    const apiKey = source.apiKey.trim();
    if (apiKey) {
      payload.api_key = apiKey;
    } else {
      delete payload.api_key;
    }
    nextSources[def.id] = payload;
  }

  nextSearchTool.sources = nextSources;
  result.search_tool = nextSearchTool as AppSettings["search_tool"];
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

function defaultSearchSourceForms(): Record<SearchSourceId, SearchSourceForm> {
  return {
    exa: defaultSearchSourceForm(),
    tavily: defaultSearchSourceForm(),
    grok: defaultSearchSourceForm(),
    brave: defaultSearchSourceForm(),
  };
}

function defaultSearchSourceForm(): SearchSourceForm {
  return {
    enabled: false,
    apiKey: "",
  };
}

function searchSourceLabel(sourceId: SearchSourceId): string {
  return (
    SEARCH_SOURCE_DEFS.find((source) => source.id === sourceId)?.label ??
    sourceId
  );
}

function searchTestQueryForLocale(locale: string): string {
  return locale.toLowerCase().startsWith("zh")
    ? "今天有什么新闻"
    : "today's news";
}

function withoutSearchSourceKey<T>(
  values: Partial<Record<SearchSourceId, T>>,
  sourceId: SearchSourceId,
): Partial<Record<SearchSourceId, T>> {
  const next = { ...values };
  delete next[sourceId];
  return next;
}

function searchMaxResultsInput(value: unknown): string {
  if (typeof value === "number" && Number.isFinite(value)) {
    return String(clampSearchMaxResults(value));
  }
  return DEFAULT_SEARCH_MAX_RESULTS;
}

function normalizedSearchMaxResults(value: string): number {
  const parsed = Number.parseInt(value, 10);
  if (!Number.isFinite(parsed)) {
    return Number(DEFAULT_SEARCH_MAX_RESULTS);
  }
  return clampSearchMaxResults(parsed);
}

function clampSearchMaxResults(value: number): number {
  return Math.min(
    SEARCH_MAX_RESULTS_MAX,
    Math.max(SEARCH_MAX_RESULTS_MIN, Math.floor(value)),
  );
}

function searchContextSizeValue(value: unknown): SearchContextSize {
  return value === "low" || value === "medium" || value === "high"
    ? value
    : DEFAULT_SEARCH_CONTEXT_SIZE;
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

function isApiBridgeRetryFormReady(form: ApiBridgeRetryFormState): boolean {
  if (
    !form.retry429Unlimited &&
    !isIntegerSettingReady(form.retry429MaxRetries, 0)
  ) {
    return false;
  }
  return isIntegerSettingReady(form.retry429DelaySeconds, 1);
}

function serializeApiBridgeRetryForm(form: ApiBridgeRetryFormState): string {
  return JSON.stringify({
    enabled: form.retry429Enabled,
    max_retries: form.retry429Unlimited
      ? null
      : parseIntegerSetting(form.retry429MaxRetries, 10, 0),
    delay_seconds: parseIntegerSetting(form.retry429DelaySeconds, 10, 1),
  });
}

function isIntegerSettingReady(value: string, min: number): boolean {
  const trimmed = value.trim();
  if (!/^\d+$/.test(trimmed)) return false;
  return Number.parseInt(trimmed, 10) >= min;
}

function parseIntegerSetting(value: string, fallback: number, min: number): number {
  const parsed = Number.parseInt(value.trim(), 10);
  if (!Number.isFinite(parsed)) return fallback;
  return Math.max(min, parsed);
}

function managedPluginKey(category: ManagedPluginCategory, id: string): string {
  return `${category}:${id}`;
}

function mergeManagedPlugins(
  previous: ManagedPluginSummary[],
  incoming: ManagedPluginSummary[],
): ManagedPluginSummary[] {
  const merged = new Map(
    previous.map((plugin) => [
      managedPluginKey(plugin.category, plugin.id),
      plugin,
    ]),
  );
  for (const plugin of incoming) {
    merged.set(managedPluginKey(plugin.category, plugin.id), plugin);
  }
  return Array.from(merged.values()).sort((left, right) =>
    left.name.localeCompare(right.name),
  );
}

function pluginCategoryLabel(
  category: Exclude<PluginCategoryFilter, "all">,
): string {
  switch (category) {
    case "acp":
      return "ACP plugin";
    case "im":
      return "IM plugin";
    case "search":
      return "Search plugin";
  }
}

function pluginActionLabel(item: PluginInventoryItem): string {
  if (item.status === "outdated") return "Update";
  return item.installed ? "Refresh" : "Install";
}

function pluginStatus(item: PluginInventoryItem): {
  label: string;
  className: string;
} {
  switch (item.status) {
    case "outdated":
      return {
        label: "Needs update",
        className: "border-amber-500/30 bg-amber-500/10 text-amber-700",
      };
    case "missing":
      return {
        label: "Not installed",
        className: "border-muted-foreground/20 bg-muted/40 text-muted-foreground",
      };
    case "ok":
      return {
        label: item.installed ? "Up to date" : "Not installed",
        className: item.installed
          ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-700"
          : "border-muted-foreground/20 bg-muted/40 text-muted-foreground",
      };
  }
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
