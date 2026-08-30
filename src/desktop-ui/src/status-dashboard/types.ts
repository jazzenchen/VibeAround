import type { useI18n } from "@va/i18n";

import type { AgentRuntime } from "../hooks/useAgentsRuntime";
import type { ChannelRuntime } from "../hooks/useChannelsState";
import type { TunnelRuntime } from "../hooks/useTunnelsState";

export type Translate = ReturnType<typeof useI18n>["t"];
export type Tone = "good" | "busy" | "warning" | "danger" | "muted";

export interface RuntimeStateProps {
  channels: {
    channels: ChannelRuntime[];
    start: (kind: string) => unknown;
    stop: (kind: string) => unknown;
    restart: (kind: string) => unknown;
  };
  tunnels: {
    tunnels: TunnelRuntime[];
    kill: (provider: string) => unknown;
  };
  agents: {
    agents: AgentRuntime[];
    error: string | null;
    loading: boolean;
    everLoaded: boolean;
    refresh: () => unknown;
  };
}
