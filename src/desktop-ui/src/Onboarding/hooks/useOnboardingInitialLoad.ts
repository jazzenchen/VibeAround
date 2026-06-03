import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

import {
  hydrateAgents,
  hydrateChannels,
  hydrateStartkitPrefs,
  hydrateTunnel,
} from "../lib/hydrateSettings";
import { listCatalog, listProfiles } from "../../Launch/api";
import type { CatalogEntry, ProfileSummary } from "../../Launch/types";
import type { AgentId, TunnelProvider } from "../constants";
import type {
  AgentSummary,
  ChannelVerboseConfig,
  DiscoveredChannelPlugin,
  PluginRegistryEntry,
  Settings,
  StartkitManifestSummary,
  TunnelSummary,
} from "../types";

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

export function useOnboardingInitialLoad({
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
}: {
  setSettings: (value: Settings) => void;
  setLoaded: (value: boolean) => void;
  setManifest: (value: StartkitManifestSummary) => void;
  setCatalog: (value: CatalogEntry[]) => void;
  setProfiles: (value: ProfileSummary[]) => void;
  setAgents: (value: AgentSummary[]) => void;
  setTunnels: (value: TunnelSummary[]) => void;
  setPluginRegistry: (value: PluginRegistryEntry[]) => void;
  setDiscoveredPlugins: (value: DiscoveredChannelPlugin[]) => void;
  setDownloadSource: (value: string) => void;
  setToolchainMode: (value: "auto" | "managed" | "system") => void;
  setShellPath: (value: boolean) => void;
  setEnabledAgents: (value: Set<AgentId>) => void;
  setEnabledChannels: (value: Set<string>) => void;
  setChannelConfigs: (value: Record<string, Record<string, string>>) => void;
  setChannelVerbose: (value: Record<string, ChannelVerboseConfig>) => void;
  setTunnelProvider: (value: TunnelProvider) => void;
  setNgrokToken: (value: string) => void;
  setNgrokDomain: (value: string) => void;
  setCfToken: (value: string) => void;
  setCfHostname: (value: string) => void;
}) {
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
          setCatalog(catalogDefs);
          setProfiles(profileDefs);

          hydrateStartkitPrefs(loadedSettings, {
            setDownloadSource,
            setToolchainMode,
            setShellPath,
          });
          hydrateAgents(loadedSettings, orderedAgents, setEnabledAgents);
          hydrateChannels(loadedSettings, pluginDefs, {
            setEnabledChannels,
            setChannelConfigs,
            setChannelVerbose,
          });
          hydrateTunnel(loadedSettings, {
            setTunnelProvider,
            setNgrokToken,
            setNgrokDomain,
            setCfToken,
            setCfHostname,
          });

          setLoaded(true);
        },
      )
      .catch((error) => {
        console.error("failed to load onboarding data", error);
        setLoaded(true);
      });
  }, [
    setAgents,
    setCatalog,
    setCfHostname,
    setCfToken,
    setChannelConfigs,
    setChannelVerbose,
    setDiscoveredPlugins,
    setDownloadSource,
    setEnabledAgents,
    setEnabledChannels,
    setLoaded,
    setManifest,
    setNgrokDomain,
    setNgrokToken,
    setPluginRegistry,
    setProfiles,
    setSettings,
    setShellPath,
    setToolchainMode,
    setTunnelProvider,
    setTunnels,
  ]);
}

function orderAgents(agentDefs: AgentSummary[]): AgentSummary[] {
  const rank = new Map(AGENT_DISPLAY_ORDER.map((id, index) => [id, index]));
  return [...agentDefs].sort(
    (a, b) => (rank.get(a.id) ?? 999) - (rank.get(b.id) ?? 999),
  );
}
