import { Globe, KeyRound, Trash2 } from "lucide-react";

import { BrandIcon } from "@/components/brand-icon";
import { Button } from "@/components/ui/button";

import { StepChannels } from "./StepChannels";
import { StepTunnel } from "./StepTunnel";
import { PanelSection } from "./PanelSection";
import type { ProfileSummary } from "../../Launch/types";
import type {
  AuthFlowState,
  ChannelVerboseConfig,
  DiscoveredChannelPlugin,
  PluginRegistryEntry,
  TunnelSummary,
} from "../types";
import type { TunnelProvider } from "../constants";

export function ConfigurePanel({
  profiles,
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
  onCreateProfile,
  onDeleteProfile,
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
  profiles: ProfileSummary[];
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
  onCreateProfile: () => void;
  onDeleteProfile: (id: string) => void;
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
  const showNoConfig =
    enabledChannels.size === 0 && tunnelProvider === "none" && profiles.length === 0;

  return (
    <div className="mx-auto flex min-h-full w-full max-w-4xl items-center py-8">
      <div className="w-full space-y-4">
        {!showNoConfig && (
          <section className="rounded-md border border-border bg-card p-4">
            <div className="flex items-center justify-between gap-3">
              <div className="min-w-0">
                <div className="flex items-center gap-2 text-sm font-medium">
                  <KeyRound className="h-4 w-4 text-primary" />
                  Agent API profiles
                </div>
                <p className="mt-1 text-xs text-muted-foreground">
                  Optional. You can add or edit profiles from Launch later.
                </p>
              </div>
              <Button type="button" size="sm" variant="outline" onClick={onCreateProfile}>
                Add profile
              </Button>
            </div>
            {profiles.length > 0 && (
              <div className="mt-3">
                <ProfileList profiles={profiles} onDeleteProfile={onDeleteProfile} />
              </div>
            )}
          </section>
        )}

        {enabledChannels.size > 0 && (
          <StepChannels
            pluginRegistry={pluginRegistry}
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
            description="Finish credentials and QR login for selected IM plugins."
          />
        )}

        {tunnelProvider !== "none" && (
          <PanelSection
            icon={<Globe className="h-4 w-4" />}
            title="Remote access configuration"
            description="Paste tunnel details when remote access was selected."
          >
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
          </PanelSection>
        )}

        {showNoConfig && (
          <div className="rounded-md border border-border bg-card px-4 py-10 text-center">
            <CheckReady />
            <div className="mt-3 text-sm font-medium">No extra configuration</div>
            <p className="mt-1 text-xs text-muted-foreground">
              The selected setup can launch now.
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

function ProfileList({
  profiles,
  onDeleteProfile,
}: {
  profiles: ProfileSummary[];
  onDeleteProfile: (id: string) => void;
}) {
  if (profiles.length === 0) {
    return (
      <div className="rounded-md border border-dashed border-border px-3 py-6 text-center text-xs text-muted-foreground">
        No API profiles yet.
      </div>
    );
  }

  return (
    <div className="grid grid-cols-[repeat(auto-fill,minmax(210px,1fr))] gap-2">
      {profiles.map((profile) => (
        <div
          key={profile.id}
          className="flex min-h-[58px] items-center gap-2 rounded-md border border-border bg-background p-2"
        >
          <BrandIcon
            kind="provider"
            id={profile.provider}
            label={profile.providerLabel}
            fallback={profile.providerIcon}
            className="h-8 w-8"
          />
          <span className="min-w-0 flex-1">
            <span className="block truncate text-[13px] font-medium">
              {profile.label}
            </span>
            <span className="block truncate text-[10px] text-muted-foreground">
              {profile.providerLabel}
            </span>
          </span>
          <Button
            type="button"
            variant="ghost"
            size="icon-xs"
            className="h-7 w-7 shrink-0 text-muted-foreground hover:text-destructive"
            onClick={() => onDeleteProfile(profile.id)}
          >
            <Trash2 className="h-3.5 w-3.5" />
          </Button>
        </div>
      ))}
    </div>
  );
}
