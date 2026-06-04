import { Globe, KeyRound } from "lucide-react";
import { useI18n } from "@va/i18n";

import { StepChannels } from "./StepChannels";
import { StepTunnel } from "./StepTunnel";
import type {
  AuthFlowState,
  ChannelVerboseConfig,
  DiscoveredChannelPlugin,
  PluginRegistryEntry,
  TunnelSummary,
} from "../types";
import type { TunnelProvider } from "../constants";

export function ConfigurePanel({
  enabledChannels,
  tunnelProvider,
  pluginRegistry,
  discoveredPlugins,
  channelConfigs,
  channelVerbose,
  installingPlugins,
  authStates,
  tunnels,
  ngrokToken,
  ngrokDomain,
  cfToken,
  cfHostname,
  finishError,
  onToggleChannel,
  onConfigChange,
  onVerboseChange,
  onInstallPlugin,
  onStartAuth,
  onCancelAuth,
  onProvider,
  onNgrokToken,
  onNgrokDomain,
  onCfToken,
  onCfHostname,
}: {
  enabledChannels: Set<string>;
  tunnelProvider: TunnelProvider;
  pluginRegistry: PluginRegistryEntry[];
  discoveredPlugins: DiscoveredChannelPlugin[];
  channelConfigs: Record<string, Record<string, string>>;
  channelVerbose: Record<string, ChannelVerboseConfig>;
  installingPlugins: Set<string>;
  authStates: Record<string, AuthFlowState>;
  tunnels: TunnelSummary[];
  ngrokToken: string;
  ngrokDomain: string;
  cfToken: string;
  cfHostname: string;
  finishError: string | null;
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
  onProvider: (value: TunnelProvider) => void;
  onNgrokToken: (value: string) => void;
  onNgrokDomain: (value: string) => void;
  onCfToken: (value: string) => void;
  onCfHostname: (value: string) => void;
}) {
  const { t } = useI18n();
  const selectedPluginRegistry = pluginRegistry.filter((entry) =>
    enabledChannels.has(entry.id),
  );
  const hasMessagingConfig = selectedPluginRegistry.length > 0;
  const hasRemoteConfig = tunnelProvider !== "none";
  const showNoConfig = !hasMessagingConfig && !hasRemoteConfig;

  return (
    <div className="mx-auto flex min-h-full w-full max-w-4xl items-center py-4">
      <div className="w-full space-y-8">
        {hasMessagingConfig && (
          <StepChannels
            pluginRegistry={selectedPluginRegistry}
            discoveredPlugins={discoveredPlugins}
            enabledChannels={enabledChannels}
            channelConfigs={channelConfigs}
            channelVerbose={channelVerbose}
            installingPlugins={installingPlugins}
            authStates={authStates}
            onToggleChannel={onToggleChannel}
            onConfigChange={onConfigChange}
            onVerboseChange={onVerboseChange}
            onInstallPlugin={onInstallPlugin}
            onStartAuth={onStartAuth}
            onCancelAuth={onCancelAuth}
            switchSize="sm"
            description={t("Finish credentials and QR login for selected messaging apps.")}
          />
        )}

        {hasRemoteConfig && (
          <section
            className={[
              "space-y-3 px-1",
              hasMessagingConfig ? "border-t border-border pt-6" : "",
            ].join(" ")}
          >
            <div>
              <div className="flex items-center gap-2 text-base font-semibold">
                <Globe className="h-4 w-4 text-primary" />
                {t("Remote access configuration")}
              </div>
              <p className="mt-1 text-sm text-muted-foreground">
                {t("Paste tunnel details when remote access was selected.")}
              </p>
            </div>
            <StepTunnel
              tunnels={tunnels}
              provider={tunnelProvider}
              onProvider={onProvider}
              ngrokToken={ngrokToken}
              onNgrokToken={onNgrokToken}
              ngrokDomain={ngrokDomain}
              onNgrokDomain={onNgrokDomain}
              cfToken={cfToken}
              onCfToken={onCfToken}
              cfHostname={cfHostname}
              onCfHostname={onCfHostname}
            />
          </section>
        )}

        {showNoConfig && (
          <div className="px-4 py-10 text-center">
            <CheckReady />
            <div className="mt-3 text-sm font-medium">{t("No extra configuration")}</div>
            <p className="mt-1 text-xs text-muted-foreground">
              {t("The selected setup can launch now.")}
            </p>
          </div>
        )}

        {finishError && (
          <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive">
            {finishError}
          </div>
        )}
      </div>
    </div>
  );
}

function CheckReady() {
  return (
    <div className="mx-auto flex h-10 w-10 items-center justify-center rounded-md bg-primary/10 text-primary">
      <KeyRound className="h-5 w-5" />
    </div>
  );
}
