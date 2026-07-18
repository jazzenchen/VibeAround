import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { formatErrorMessage } from "@va/client";

import { firstSupportedAuthMethod } from "../authMethods";
import type { AuthFlowState, DiscoveredChannelPlugin } from "../types";

interface UseChannelAuthInput {
  active?: boolean;
  discoveredPlugins: DiscoveredChannelPlugin[];
  channelConfigs: Record<string, Record<string, string>>;
  onConfigChange: (pluginId: string, key: string, value: string) => void;
}

interface UseChannelAuthResult {
  authStates: Record<string, AuthFlowState>;
  startAuth: (
    pluginId: string,
    params?: Record<string, unknown>,
  ) => Promise<void>;
  cancelAuth: (pluginId: string) => Promise<void>;
}

/**
 * Owns the interactive auth state for channel plugins.
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
  const activeOperationsRef = useRef(new Map<string, object>());
  const keepAuthAlive = active === true;

  useEffect(() => {
    authStatesRef.current = authStates;
  }, [authStates]);

  const startAuth = useCallback(
    async (pluginId: string, params: Record<string, unknown> = {}) => {
      const operation = {};
      activeOperationsRef.current.set(pluginId, operation);
      const isCurrent = () => activeOperationsRef.current.get(pluginId) === operation;

      setAuthStates((prev) => ({
        ...prev,
        [pluginId]: { status: "generating", message: "Connecting..." },
      }));

      try {
        const discovered = discoveredPlugins.find((p) => p.id === pluginId);
        const authMethod = firstSupportedAuthMethod(
          discovered?.capabilities.auth?.methods,
        );
        if (!authMethod) throw new Error("This plugin has no supported authentication method.");

        const schemaProps = discovered?.configSchema?.properties ?? {};
        const configForAuth = Object.fromEntries(
          Object.entries(schemaProps).map(([key, property]) => [
            key,
            channelConfigs[pluginId]?.[key] ?? property.default ?? "",
          ]),
        );

        const result = await invoke<Record<string, unknown>>("plugin_auth_start", {
          request: {
            pluginId,
            params: authMethod === "qrcode_login" ? configForAuth : params,
          },
        });
        if (!isCurrent()) return;

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
        const pairingCode = result.pairingCode as string | undefined;
        const hasChallenge = authMethod === "qrcode_login" ? !!qrUrl : !!pairingCode;
        setAuthStates((prev) => ({
          ...prev,
          [pluginId]: {
            status: hasChallenge ? "waiting" : "error",
            message: String(
              result.message ??
                (authMethod === "qrcode_login"
                  ? "Scan the QR code."
                  : "Enter the pairing code on your phone."),
            ),
            qrCodeUrl: qrUrl,
            pairingCode,
            sessionKey: result.sessionKey as string | undefined,
          },
        }));

        if (!hasChallenge) return;

        try {
          const waitResult = await invoke<Record<string, unknown>>("plugin_auth_wait", {
            request: {
              pluginId,
              params:
                authMethod === "qrcode_login"
                  ? { sessionKey: result.sessionKey, timeoutMs: 480000 }
                  : {},
            },
          });
          if (!isCurrent()) return;

          if (waitResult.connected) {
            setAuthStates((prev) => ({
              ...prev,
              [pluginId]: {
                status: "connected",
                message: formatErrorMessage(
                  waitResult.message,
                  "Connected successfully.",
                ),
              },
            }));
            if (waitResult.botToken) onConfigChange(pluginId, "bot_token", String(waitResult.botToken));
            if (waitResult.accountId) onConfigChange(pluginId, "account_id", String(waitResult.accountId));
          } else {
            setAuthStates((prev) => ({
              ...prev,
              [pluginId]: {
                status: "idle",
                message: formatErrorMessage(waitResult.message, "Not confirmed."),
              },
            }));
          }
        } catch {
          if (!isCurrent()) return;
          setAuthStates((prev) => ({
            ...prev,
            [pluginId]: { status: "error", message: "Connection lost. Try again." },
          }));
        }
      } catch (error) {
        if (!isCurrent()) return;
        setAuthStates((prev) => ({
          ...prev,
          [pluginId]: {
            status: "error",
            message: formatErrorMessage(error),
          },
        }));
      }
    },
    [discoveredPlugins, channelConfigs, onConfigChange],
  );

  const cancelAuth = useCallback(async (pluginId: string) => {
    activeOperationsRef.current.delete(pluginId);
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
        activeOperationsRef.current.delete(pluginId);
        void invoke("plugin_auth_cancel", { request: { pluginId } }).catch(() => {});
      }
    }
  }, [keepAuthAlive, authStates]);

  useEffect(() => {
    return () => {
      for (const [pluginId, state] of Object.entries(authStatesRef.current)) {
        if (state.status === "generating" || state.status === "waiting") {
          activeOperationsRef.current.delete(pluginId);
          void invoke("plugin_auth_cancel", { request: { pluginId } }).catch(() => {});
        }
      }
    };
  }, []);

  return { authStates, startAuth, cancelAuth };
}
