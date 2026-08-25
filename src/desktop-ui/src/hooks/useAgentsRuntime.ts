import {
  AgentRuntimeListSchema,
  type AgentRuntime,
} from "@va/client";
import { useManagerState } from "./useManagerState";

export type { AgentRuntime };

/**
 * Agents tab in the desktop dashboard. Subscribes to
 * `/ws/agents/runtime` for live updates and falls back to
 * `/api/agents/runtime` polling on disconnect.
 */
export function useAgentsRuntime() {
  const base = useManagerState(
    "/api/agents/runtime",
    "/ws/agents/runtime",
    AgentRuntimeListSchema,
  );

  return {
    agents: base.data,
    error: base.error,
    loading: base.loading,
    connected: base.connected,
    everLoaded: base.everLoaded,
    refresh: base.refresh,
  };
}
