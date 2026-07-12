import { useCallback } from "react";
import {
  ChannelRuntimeListSchema,
  type ChannelRuntime,
} from "@va/client";
import { apiFetch } from "../lib/api";
import { useManagerState } from "./useManagerState";

export type { ChannelRuntime };

/**
 * Channels tab in the desktop dashboard. Subscribes to `/ws/channels`
 * for live updates and falls back to `/api/channels` polling on
 * disconnect. Exposes the stop/start/restart actions backed by the
 * `/api/channels/:instance_id/:action` endpoints.
 */
export function useChannelsState() {
  const base = useManagerState(
    "/api/channels",
    "/ws/channels",
    ChannelRuntimeListSchema,
  );

  const action = useCallback(
    async (instanceId: string, verb: "start" | "stop" | "restart") => {
      try {
        const res = await apiFetch(
          `/api/channels/${encodeURIComponent(instanceId)}/${verb}`,
          { method: "POST" },
        );
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        if (!base.connected) await base.refresh();
      } catch (e) {
        console.warn(`[useChannelsState] ${verb} ${instanceId} failed:`, e);
      }
    },
    [base],
  );

  return {
    channels: base.data,
    error: base.error,
    loading: base.loading,
    connected: base.connected,
    everLoaded: base.everLoaded,
    refresh: base.refresh,
    start: (instanceId: string) => action(instanceId, "start"),
    stop: (instanceId: string) => action(instanceId, "stop"),
    restart: (instanceId: string) => action(instanceId, "restart"),
  };
}
