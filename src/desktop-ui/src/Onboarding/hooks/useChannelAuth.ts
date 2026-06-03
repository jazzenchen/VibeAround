import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import type { AuthFlowState, DiscoveredChannelPlugin } from "../types";

interface UseChannelAuthInput {
  active?: boolean;
  discoveredPlugins: DiscoveredChannelPlugin[];
  channelConfigs: Record<string, Record<string, string>>;
  onConfigChange: (pluginId: string, key: string, value: string) => void;
}

interface UseChannelAuthResult {
  authStates: Record<string, AuthFlowState>;
  startAuth: (pluginId: string) => Promise<void>;
  cancelAuth: (pluginId: string) => Promise<void>;
}

/**
 * Owns the QR-login auth state machine for channel plugins.
 *
 * Each plugin goes through: generating -> waiting -> connected / error / idle.
 * Also cancels any in-flight auth session if the user navigates away from
 * the active configuration step; otherwise the plugin process would keep polling.
 */
export function useChannelAuth({
  active,
  discoveredPlugins,
  channelConfigs,
  onConfigChange,
}: UseChannelAuthInput): UseChannelAuthResult {
  const [authStates, setAuthStates] = useState<Record<string, AuthFlowState>>({});
  const authStatesRef = useRef(authStates);
  const keepAuthAlive = active === true;

  useEffect(() => {
    authStatesRef.current = authStates;
  }, [authStates]);

  const startAuth = useCallback(
    async (pluginId: string) => {
      setAuthStates((prev) => ({
        ...prev,
        [pluginId]: { status: "generating", message: "Connecting..." },
      }));

      try {
        const discovered = discoveredPlugins.find((p) => p.id === pluginId);
        const schemaProps = discovered?.configSchema?.properties ?? {};
        const configForAuth: Record<string, string> = {};
        for (const [key, prop] of Object.entries(schemaProps)) {
          configForAuth[key] = channelConfigs[pluginId]?.[key] ?? prop.default ?? "";
        }

        const result = await invoke<Record<string, unknown>>("plugin_auth_start", {
          request: { pluginId, config: configForAuth },
        });

        if (result.alreadyConnected) {
          setAuthStates((prev) => ({
            ...prev,
            [pluginId]: {
              status: "connected",
              message: String(result.message ?? "Already authenticated."),
            },
          }));
          if (result.botToken) onConfigChange(pluginId, "bot_token", String(result.botToken));
          if (result.accountId) onConfigChange(pluginId, "account_id", String(result.accountId));
          return;
        }

        const qrUrl = result.qrcodeUrl as string | undefined;
        setAuthStates((prev) => ({
          ...prev,
          [pluginId]: {
            status: qrUrl ? "waiting" : "error",
            message: String(result.message ?? "Scan the QR code."),
            qrCodeUrl: qrUrl,
            sessionKey: result.sessionKey as string | undefined,
          },
        }));

        if (!qrUrl) return;

        try {
          const waitResult = await invoke<Record<string, unknown>>("plugin_auth_wait", {
            request: {
              pluginId,
              params: {
                sessionKey: result.sessionKey,
                timeoutMs: 480000,
              },
            },
          });

          if (waitResult.connected) {
            setAuthStates((prev) => ({
              ...prev,
              [pluginId]: {
                status: "connected",
                message: String(waitResult.message ?? "Connected successfully."),
              },
            }));
            if (waitResult.botToken) onConfigChange(pluginId, "bot_token", String(waitResult.botToken));
            if (waitResult.accountId) onConfigChange(pluginId, "account_id", String(waitResult.accountId));
          } else {
            setAuthStates((prev) => ({
              ...prev,
              [pluginId]: {
                status: "idle",
                message: String(waitResult.message ?? "Not confirmed."),
              },
            }));
          }
        } catch {
          setAuthStates((prev) => ({
            ...prev,
            [pluginId]: { status: "error", message: "Connection lost. Try again." },
          }));
        }
      } catch (error) {
        setAuthStates((prev) => ({
          ...prev,
          [pluginId]: {
            status: "error",
            message: error instanceof Error ? error.message : String(error),
          },
        }));
      }
    },
    [discoveredPlugins, channelConfigs, onConfigChange],
  );

  const cancelAuth = useCallback(async (pluginId: string) => {
    setAuthStates((prev) => ({
      ...prev,
      [pluginId]: { status: "idle", message: "Cancelled." },
    }));
    try {
      await invoke("plugin_auth_cancel", { request: { pluginId } });
    } catch {
      // ignore
    }
  }, []);

  useEffect(() => {
    if (keepAuthAlive) return;
    for (const [pluginId, state] of Object.entries(authStates)) {
      if (state.status === "generating" || state.status === "waiting") {
        void invoke("plugin_auth_cancel", { request: { pluginId } }).catch(() => {});
      }
    }
  }, [keepAuthAlive, authStates]);

  useEffect(() => {
    return () => {
      for (const [pluginId, state] of Object.entries(authStatesRef.current)) {
        if (state.status === "generating" || state.status === "waiting") {
          void invoke("plugin_auth_cancel", { request: { pluginId } }).catch(() => {});
        }
      }
    };
  }, []);

  return { authStates, startAuth, cancelAuth };
}
