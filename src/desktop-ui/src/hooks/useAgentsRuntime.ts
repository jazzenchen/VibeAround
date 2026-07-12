import { useCallback } from "react";
import {
  AgentRuntimeListSchema,
  type AgentRuntime,
} from "@va/client";
import { apiFetch } from "../lib/api";
import { useManagerState } from "./useManagerState";

export type { AgentRuntime };

/**
 * Agents tab in the desktop dashboard. Subscribes to
 * `/ws/agents/runtime` for live updates and falls back to
 * `/api/agents/runtime` polling on disconnect. `kill` stops the live
 * host through `DELETE /api/agents/:thread_id`, using the workspace thread id
 * returned by the runtime API.
 */
export function useAgentsRuntime() {
  const base = useManagerState(
    "/api/agents/runtime",
    "/ws/agents/runtime",
    AgentRuntimeListSchema,
  );

  const kill = useCallback(
    async (routeKey: string) => {
      try {
        const res = await apiFetch(
          `/api/agents/${encodeURIComponent(routeKey)}`,
          { method: "DELETE" },
        );
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        if (!base.connected) await base.refresh();
      } catch (e) {
        console.warn(`[useAgentsRuntime] kill ${routeKey} failed:`, e);
      }
    },
    [base],
  );

  return {
    agents: base.data,
    error: base.error,
    loading: base.loading,
    connected: base.connected,
    everLoaded: base.everLoaded,
    refresh: base.refresh,
    kill,
  };
}
