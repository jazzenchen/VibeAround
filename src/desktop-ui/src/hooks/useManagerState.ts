import { useCallback, useEffect, useRef, useState } from "react";
import { z } from "zod";
import { formatErrorMessage } from "@va/client";
import { apiFetch, authedWsUrl } from "../lib/api";

const POLL_INTERVAL_MS = 5000;
const RECONCILE_INTERVAL_MS = 15000;
const WS_RECONNECT_DELAY_MS = 3000;

/**
 * Common state-subscription primitive for every per-domain hook
 * (`useChannelsState`, `useTunnelsState`, `useAgentsRuntime`, ...).
 *
 * Opens `wsPath`, reconciles through `httpPath`, and polls while disconnected.
 *
 * `schema` validates every wire frame — bad payloads get logged and
 * skipped rather than silently putting garbage into React state.
 */
export function useManagerState<T>(
  httpPath: string,
  wsPath: string,
  schema: z.ZodType<T[]>,
) {
  const [data, setData] = useState<T[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [connected, setConnected] = useState(false);
  const [everLoaded, setEverLoaded] = useState(false);
  const wsRef = useRef<WebSocket | null>(null);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const reconcileRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const mountedRef = useRef(false);

  const fetchOnce = useCallback(async () => {
    try {
      const res = await apiFetch(httpPath);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const parsed = schema.parse(await res.json());
      setData(parsed);
      setError(null);
      setEverLoaded(true);
    } catch (e) {
      setError(formatErrorMessage(e, "fetch failed"));
    } finally {
      setLoading(false);
    }
  }, [httpPath, schema]);

  const startPolling = useCallback(() => {
    if (pollRef.current) return;
    void fetchOnce();
    pollRef.current = setInterval(() => void fetchOnce(), POLL_INTERVAL_MS);
  }, [fetchOnce]);

  const stopPolling = useCallback(() => {
    if (pollRef.current) {
      clearInterval(pollRef.current);
      pollRef.current = null;
    }
  }, []);

  const startReconcile = useCallback(() => {
    if (reconcileRef.current) return;
    reconcileRef.current = setInterval(
      () => void fetchOnce(),
      RECONCILE_INTERVAL_MS,
    );
  }, [fetchOnce]);

  const stopReconcile = useCallback(() => {
    if (reconcileRef.current) {
      clearInterval(reconcileRef.current);
      reconcileRef.current = null;
    }
  }, []);

  const connectWs = useCallback(async () => {
    if (!mountedRef.current) return;
    const current = wsRef.current;
    if (
      current &&
      current.readyState !== WebSocket.CLOSED &&
      current.readyState !== WebSocket.CLOSING
    ) {
      return;
    }

    const url = await authedWsUrl(wsPath);
    if (!mountedRef.current) return;
    const latest = wsRef.current;
    if (
      latest &&
      latest.readyState !== WebSocket.CLOSED &&
      latest.readyState !== WebSocket.CLOSING
    ) {
      return;
    }

    const ws = new WebSocket(url);
    wsRef.current = ws;

    ws.onopen = () => {
      if (wsRef.current !== ws) return;
      setConnected(true);
      setError(null);
      stopPolling();
      void fetchOnce();
      startReconcile();
    };

    ws.onmessage = (event) => {
      if (wsRef.current !== ws) return;
      try {
        const parsed = schema.parse(JSON.parse(event.data));
        setData(parsed);
        setError(null);
        setLoading(false);
        setEverLoaded(true);
      } catch (e) {
        console.warn(`[${wsPath}] bad frame:`, e);
      }
    };

    ws.onclose = () => {
      if (wsRef.current !== ws) return;
      stopReconcile();
      setConnected(false);
      wsRef.current = null;
      startPolling();
      // Cleanup cancels pending reconnects.
      reconnectTimerRef.current = setTimeout(() => {
        reconnectTimerRef.current = null;
        void connectWs();
      }, WS_RECONNECT_DELAY_MS);
    };

    ws.onerror = () => {
      // onclose fires after this; no extra handling needed
    };
  }, [
    wsPath,
    schema,
    fetchOnce,
    startPolling,
    stopPolling,
    startReconcile,
    stopReconcile,
  ]);

  useEffect(() => {
    mountedRef.current = true;
    void connectWs();
    return () => {
      mountedRef.current = false;
      stopPolling();
      stopReconcile();
      if (reconnectTimerRef.current) {
        clearTimeout(reconnectTimerRef.current);
        reconnectTimerRef.current = null;
      }
      if (wsRef.current) {
        wsRef.current.onclose = null;
        wsRef.current.close();
        wsRef.current = null;
      }
    };
  }, [connectWs, stopPolling]);

  return { data, error, loading, connected, everLoaded, refresh: fetchOnce };
}
