import { parseChannelVerbose } from "./channelConfig";
import type { AgentId, TunnelProvider } from "../constants";
import type {
  AgentSummary,
  ChannelVerboseConfig,
  PluginRegistryEntry,
  Settings,
} from "../types";

const DEFAULT_ENABLED_AGENT_IDS = new Set<AgentId>(["claude", "codex"]);

export function hydrateStartkitPrefs(
  loadedSettings: Settings,
  setters: {
    setDownloadSource: (value: string) => void;
    setToolchainMode: (value: "auto" | "managed" | "system") => void;
    setShellPath: (value: boolean) => void;
  },
) {
  if (loadedSettings.startkit?.source) {
    setters.setDownloadSource(loadedSettings.startkit.source);
  }
  if (
    loadedSettings.startkit?.toolchain_mode === "auto" ||
    loadedSettings.startkit?.toolchain_mode === "managed" ||
    loadedSettings.startkit?.toolchain_mode === "system"
  ) {
    setters.setToolchainMode(loadedSettings.startkit.toolchain_mode);
  }
  if (typeof loadedSettings.startkit?.shell_path === "boolean") {
    setters.setShellPath(loadedSettings.startkit.shell_path);
  }
}

export function hydrateAgents(
  loadedSettings: Settings,
  orderedAgents: AgentSummary[],
  setEnabledAgents: (value: Set<AgentId>) => void,
) {
  if (Array.isArray(loadedSettings.enabled_agents)) {
    setEnabledAgents(new Set(loadedSettings.enabled_agents as AgentId[]));
    return;
  }
  setEnabledAgents(
    new Set(
      orderedAgents
        .map((agent) => agent.id)
        .filter((id) => DEFAULT_ENABLED_AGENT_IDS.has(id)),
    ),
  );
}

export function hydrateChannels(
  loadedSettings: Settings,
  pluginDefs: PluginRegistryEntry[],
  setters: {
    setEnabledChannels: (value: Set<string>) => void;
    setChannelConfigs: (value: Record<string, Record<string, string>>) => void;
    setChannelVerbose: (value: Record<string, ChannelVerboseConfig>) => void;
  },
) {
  const registryPluginIds = new Set(pluginDefs.map((p) => p.id));
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

  setters.setEnabledChannels(enabled);
  setters.setChannelConfigs(configs);
  setters.setChannelVerbose(verbose);
}

export function hydrateTunnel(
  loadedSettings: Settings,
  setters: {
    setTunnelProvider: (value: TunnelProvider) => void;
    setNgrokToken: (value: string) => void;
    setNgrokDomain: (value: string) => void;
    setCfToken: (value: string) => void;
    setCfHostname: (value: string) => void;
  },
) {
  const provider = loadedSettings.tunnel?.provider;
  if (
    provider === "none" ||
    provider === "cloudflare" ||
    provider === "ngrok" ||
    provider === "localtunnel"
  ) {
    setters.setTunnelProvider(provider);
  }
  if (loadedSettings.tunnel?.ngrok?.auth_token) {
    setters.setNgrokToken(loadedSettings.tunnel.ngrok.auth_token);
  }
  if (loadedSettings.tunnel?.ngrok?.domain) {
    setters.setNgrokDomain(loadedSettings.tunnel.ngrok.domain);
  }
  if (loadedSettings.tunnel?.cloudflare?.tunnel_token) {
    setters.setCfToken(loadedSettings.tunnel.cloudflare.tunnel_token);
  }
  if (loadedSettings.tunnel?.cloudflare?.hostname) {
    setters.setCfHostname(loadedSettings.tunnel.cloudflare.hostname);
  }
}
