import type { ReactNode } from "react";
import type { AgentId, OnboardingGoal, TunnelProvider } from "./constants";
import type { ProfileSummary } from "../Launch/types";

// Resource types — returned by Tauri commands
export interface AgentSummary {
  id: string;
  display_name: string;
  description: string;
  install_type?: "npm" | "script" | "path";
}

export interface TunnelSummary {
  id: string;
  display_name: string;
}

export interface PluginRegistryEntry {
  id: string;
  kind: string;
  slug: string;
  name: string;
  description: string;
  github: string;
}

export interface Settings {
  onboarded?: boolean;
  workspaces?: string[];
  default_agent?: string;
  default_profiles?: Record<string, string>;
  enabled_agents?: string[];
  integrations?: {
    mcp_auto_install?: boolean;
    skill_auto_install?: boolean;
  };
  im_agent?: {
    auto_continue_last_session?: boolean;
  };
  proxy?: {
    enabled?: boolean;
    http_proxy?: string;
    no_proxy?: string;
  };
  api_bridge?: {
    retry_429?: {
      enabled?: boolean;
      max_retries?: number | null;
      delay_seconds?: number;
    };
  };
  startkit?: {
    source?: string;
    toolchain_mode?: "auto" | "managed" | "system" | string;
    shell_path?: boolean;
  };
  tunnel?: {
    provider?: string;
    ngrok?: { auth_token?: string; domain?: string };
    cloudflare?: { tunnel_token?: string; hostname?: string };
  };
  channels?: Record<string, Record<string, unknown>>;
  [key: string]: unknown;
}

export interface ChannelVerboseConfig {
  show_thinking: boolean;
  show_tool_use: boolean;
}

export interface PluginAuthCapabilities {
  methods: string[];
}

export interface PluginCapabilities {
  streaming: boolean;
  interactiveCards: boolean;
  reactions: boolean;
  editMessage: boolean;
  media: boolean;
  auth?: PluginAuthCapabilities;
}

export interface ConfigSchemaProperty {
  type?: string;
  description?: string;
  default?: string;
  hidden?: boolean;
}

export interface ConfigSchema {
  type?: string;
  properties?: Record<string, ConfigSchemaProperty>;
  required?: string[];
}

export interface DiscoveredChannelPlugin {
  id: string;
  name: string;
  version: string;
  kind: string;
  runtime: string;
  entry: string;
  source: "user" | "project";
  /** Directory name on disk — may differ from id when plugin.json declares a different id. */
  dirName: string;
  supportsQrcodeLogin: boolean;
  configSchema?: ConfigSchema;
  capabilities: PluginCapabilities;
}

export type PluginInstallStatus =
  | "not_installed"
  | "installing"
  | "installed_not_built"
  | "installed_not_discoverable"
  | "ready";

export type AuthFlowStatus = "idle" | "generating" | "waiting" | "connected" | "error";

export interface AuthFlowState {
  status: AuthFlowStatus;
  message: string;
  qrCodeUrl?: string;
  sessionKey?: string;
  resultData?: Record<string, unknown>;
}

export interface StepAgentsProps {
  agents: AgentSummary[];
  profiles: ProfileSummary[];
  enabled: Set<AgentId>;
  onToggle: (id: AgentId) => void;
  onCreateProfile: () => void;
  onDeleteProfile: (id: string) => void;
}

export interface StepChannelsProps {
  pluginRegistry: PluginRegistryEntry[];
  discoveredPlugins: DiscoveredChannelPlugin[];
  enabledChannels: Set<string>;
  channelConfigs: Record<string, Record<string, string>>;
  channelVerbose: Record<string, ChannelVerboseConfig>;
  installingPlugins: Set<string>;
  authStates: Record<string, AuthFlowState>;
  onToggleChannel: (pluginId: string, enabled: boolean) => void;
  onConfigChange: (pluginId: string, key: string, value: string) => void;
  onVerboseChange: (
    pluginId: string,
    key: keyof ChannelVerboseConfig,
    value: boolean,
  ) => void;
  onInstallPlugin: (pluginId: string, githubUrl: string) => void;
  onStartAuth: (pluginId: string) => void;
  onCancelAuth: (pluginId: string) => void;
  switchSize?: "sm" | "default";
  description?: ReactNode;
  notice?: ReactNode;
}

export interface StepTunnelProps {
  tunnels: TunnelSummary[];
  provider: TunnelProvider;
  onProvider: (value: TunnelProvider) => void;
  ngrokToken: string;
  onNgrokToken: (value: string) => void;
  ngrokDomain: string;
  onNgrokDomain: (value: string) => void;
  cfToken: string;
  onCfToken: (value: string) => void;
  cfHostname: string;
  onCfHostname: (value: string) => void;
  notice?: ReactNode;
}

export interface StepConfirmProps {
  agents: AgentSummary[];
  tunnels: TunnelSummary[];
  pluginRegistry: PluginRegistryEntry[];
  selectedGoals: Set<OnboardingGoal>;
  enabledAgents: Set<AgentId>;
  tunnelProvider: TunnelProvider;
  enabledChannels: Set<string>;
  // Install progress state
  isInstalling: boolean;
  installComplete: boolean;
  installTasks: InstallTaskProgress[];
}

export type InstallTaskStatus = "pending" | "running" | "done" | "error" | "skipped" | "cancelled";

export interface InstallTaskProgress {
  id: string;
  label: string;
  status: InstallTaskStatus;
  message?: string;
  logs?: string[];
}

export interface InstallTaskInfo {
  id: string;
  label: string;
}

export type StartkitStatus =
  | "pending"
  | "running"
  | "ok"
  | "missing"
  | "outdated"
  | "broken"
  | "needs_config"
  | "blocked"
  | "error"
  | "skipped";

export interface StartkitChoices {
  agents: string[];
  tunnel: string;
  channels: string[];
  source: string;
  toolchainMode: "auto" | "managed" | "system" | string;
  shellPath: boolean;
}

export interface StartkitSource {
  label: string;
  node_index: string;
  node_dist: string;
  npm_registry: string;
}

export interface StartkitItemSummary {
  id: string;
  label: string;
  group: string;
  category: string;
  description?: string;
  severity?: string;
  kind?: string;
  managed: boolean;
  hasRepair: boolean;
  secret: boolean;
  settingsKey?: string;
}

export interface StartkitManifestSummary {
  id: string;
  name: string;
  schema: number;
  version: string;
  sources: Record<string, StartkitSource>;
  items: StartkitItemSummary[];
}

export interface StartkitPlan {
  platform: string;
  source: string;
  itemIds: string[];
  items: StartkitItemSummary[];
}

export interface StartkitItemReport {
  id: string;
  label: string;
  group: string;
  category: string;
  status: StartkitStatus;
  severity?: string;
  version?: string;
  path?: string;
  message?: string;
  actions: string[];
  secret: boolean;
  settingsKey?: string;
}

export interface StartkitScanReport {
  plan: StartkitPlan;
  reports: StartkitItemReport[];
}

export interface StartkitProgressEvent {
  id: string;
  label: string;
  status: StartkitStatus;
  message?: string;
  report?: StartkitItemReport;
}

export interface StartkitCompleteEvent {
  status: string;
}
