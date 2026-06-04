import { AgentDecisionPanel } from "./AgentDecisionPanel";
import { ConfigurePanel } from "./ConfigurePanel";
import { ImDecisionPanel } from "./ImDecisionPanel";
import { InstallPanel } from "./InstallPanel";
import { RemoteDecisionPanel } from "./RemoteDecisionPanel";
import type { AgentId, TunnelProvider } from "../constants";
import type {
  AgentSummary,
  AuthFlowState,
  ChannelVerboseConfig,
  DiscoveredChannelPlugin,
  PluginRegistryEntry,
  StartkitChoices,
  StartkitItemReport,
  TunnelSummary,
} from "../types";
import type { WizardStepId } from "../wizardTypes";

export function OnboardingStepContent({
  activeStep,
  agents,
  enabledAgents,
  reportsById,
  scanning,
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
  channelConfigs,
  channelVerbose,
  installingPlugins,
  authStates,
  ngrokToken,
  ngrokDomain,
  cfToken,
  cfHostname,
  finishError,
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
  channelConfigs: Record<string, Record<string, string>>;
  channelVerbose: Record<string, ChannelVerboseConfig>;
  installingPlugins: Set<string>;
  authStates: Record<string, AuthFlowState>;
  ngrokToken: string;
  ngrokDomain: string;
  cfToken: string;
  cfHostname: string;
  finishError: string | null;
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
      className="min-h-0 overflow-y-auto p-5 animate-in fade-in slide-in-from-bottom-1 duration-300"
    >
      {activeStep === "agents" && (
        <AgentDecisionPanel
          agents={agents}
          enabledAgents={enabledAgents}
          reports={reportsById}
          scanning={scanning}
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
          pluginRegistry={pluginRegistry}
          discoveredPlugins={discoveredPlugins}
        />
      )}

      {activeStep === "configure" && (
        <ConfigurePanel
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
