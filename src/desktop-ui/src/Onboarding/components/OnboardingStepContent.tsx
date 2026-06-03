import { AgentDecisionPanel } from "./AgentDecisionPanel";
import { ConfigurePanel } from "./ConfigurePanel";
import { ImDecisionPanel } from "./ImDecisionPanel";
import { InstallPanel } from "./InstallPanel";
import { RemoteDecisionPanel } from "./RemoteDecisionPanel";
import type { ProfileSummary } from "../../Launch/types";
import type { AgentId, TunnelProvider } from "../constants";
import type {
  AgentSummary,
  AuthFlowState,
  ChannelVerboseConfig,
  DiscoveredChannelPlugin,
  PluginRegistryEntry,
  StartkitChoices,
  StartkitItemReport,
  StartkitManifestSummary,
  TunnelSummary,
} from "../types";
import type { WizardStepId } from "../wizardTypes";

export function OnboardingStepContent({
  activeStep,
  agents,
  enabledAgents,
  reportsById,
  scanning,
  toolchainMode,
  onToolchainMode,
  manifest,
  downloadSource,
  onDownloadSource,
  shellPath,
  onShellPath,
  onToggleAgent,
  pluginRegistry,
  discoveredPlugins,
  enabledChannels,
  onToggleChannel,
  tunnels,
  tunnelProvider,
  onTunnelProvider,
  groupedReports,
  reports,
  running,
  complete,
  finalStatus,
  startkitError,
  choices,
  profiles,
  channelConfigs,
  channelVerbose,
  installingPlugins,
  authStates,
  ngrokToken,
  ngrokDomain,
  cfToken,
  cfHostname,
  finishError,
  onCreateProfile,
  onDeleteProfile,
  onConfigChange,
  onVerboseChange,
  onInstallPlugin,
  onStartAuth,
  onCancelAuth,
  onNgrokToken,
  onNgrokDomain,
  onCfToken,
  onCfHostname,
}: {
  activeStep: WizardStepId;
  agents: AgentSummary[];
  enabledAgents: Set<AgentId>;
  reportsById: Map<string, StartkitItemReport>;
  scanning: boolean;
  toolchainMode: "auto" | "managed" | "system";
  onToolchainMode: (value: "auto" | "managed" | "system") => void;
  manifest: StartkitManifestSummary | null;
  downloadSource: string;
  onDownloadSource: (value: string) => void;
  shellPath: boolean;
  onShellPath: (checked: boolean) => void;
  onToggleAgent: (id: AgentId) => void;
  pluginRegistry: PluginRegistryEntry[];
  discoveredPlugins: DiscoveredChannelPlugin[];
  enabledChannels: Set<string>;
  onToggleChannel: (pluginId: string, enabled: boolean) => void;
  tunnels: TunnelSummary[];
  tunnelProvider: TunnelProvider;
  onTunnelProvider: (value: TunnelProvider) => void;
  groupedReports: Array<{ id: string; reports: StartkitItemReport[] }>;
  reports: StartkitItemReport[];
  running: boolean;
  complete: boolean;
  finalStatus: string | null;
  startkitError: string | null;
  choices: StartkitChoices;
  profiles: ProfileSummary[];
  channelConfigs: Record<string, Record<string, string>>;
  channelVerbose: Record<string, ChannelVerboseConfig>;
  installingPlugins: Set<string>;
  authStates: Record<string, AuthFlowState>;
  ngrokToken: string;
  ngrokDomain: string;
  cfToken: string;
  cfHostname: string;
  finishError: string | null;
  onCreateProfile: () => void;
  onDeleteProfile: (id: string) => void;
  onConfigChange: (pluginId: string, key: string, value: string) => void;
  onVerboseChange: (
    pluginId: string,
    key: keyof ChannelVerboseConfig,
    value: boolean,
  ) => void;
  onInstallPlugin: (pluginId: string, githubUrl: string) => void;
  onStartAuth: (pluginId: string) => void;
  onCancelAuth: (pluginId: string) => void;
  onNgrokToken: (value: string) => void;
  onNgrokDomain: (value: string) => void;
  onCfToken: (value: string) => void;
  onCfHostname: (value: string) => void;
}) {
  return (
    <section
      key={activeStep}
      className="min-h-0 overflow-y-auto p-6 animate-in fade-in slide-in-from-bottom-1 duration-300"
    >
      {activeStep === "agents" && (
        <AgentDecisionPanel
          agents={agents}
          enabledAgents={enabledAgents}
          reports={reportsById}
          scanning={scanning}
          toolchainMode={toolchainMode}
          onToolchainMode={onToolchainMode}
          sources={manifest?.sources ?? {}}
          downloadSource={downloadSource}
          onDownloadSource={onDownloadSource}
          shellPath={shellPath && toolchainMode !== "system"}
          shellPathDisabled={toolchainMode === "system"}
          onShellPath={onShellPath}
          onToggleAgent={onToggleAgent}
        />
      )}

      {activeStep === "im" && (
        <ImDecisionPanel
          pluginRegistry={pluginRegistry}
          discoveredPlugins={discoveredPlugins}
          enabledChannels={enabledChannels}
          onToggleChannel={onToggleChannel}
        />
      )}

      {activeStep === "remote" && (
        <RemoteDecisionPanel
          tunnels={tunnels}
          provider={tunnelProvider}
          onProvider={onTunnelProvider}
        />
      )}

      {activeStep === "install" && (
        <InstallPanel
          groupedReports={groupedReports}
          reports={reports}
          scanning={scanning}
          running={running}
          complete={complete}
          finalStatus={finalStatus}
          error={startkitError}
          choices={choices}
          tunnelProvider={tunnelProvider}
        />
      )}

      {activeStep === "configure" && (
        <ConfigurePanel
          profiles={profiles}
          enabledChannels={enabledChannels}
          tunnelProvider={tunnelProvider}
          pluginRegistry={pluginRegistry}
          discoveredPlugins={discoveredPlugins}
          channelConfigs={channelConfigs}
          channelVerbose={channelVerbose}
          installingPlugins={installingPlugins}
          authStates={authStates}
          tunnels={tunnels}
          ngrokToken={ngrokToken}
          ngrokDomain={ngrokDomain}
          cfToken={cfToken}
          cfHostname={cfHostname}
          finishError={finishError}
          onCreateProfile={onCreateProfile}
          onDeleteProfile={onDeleteProfile}
          onToggleChannel={onToggleChannel}
          onConfigChange={onConfigChange}
          onVerboseChange={onVerboseChange}
          onInstallPlugin={onInstallPlugin}
          onStartAuth={onStartAuth}
          onCancelAuth={onCancelAuth}
          onProvider={onTunnelProvider}
          onNgrokToken={onNgrokToken}
          onNgrokDomain={onNgrokDomain}
          onCfToken={onCfToken}
          onCfHostname={onCfHostname}
        />
      )}
    </section>
  );
}
