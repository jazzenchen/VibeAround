import { useState, useEffect, useCallback, useRef } from "react";

const API_BASE = "http://127.0.0.1:12358";
const WS_URL = "ws://127.0.0.1:12358/ws/services";
const POLL_INTERVAL = 5000;
const WS_RECONNECT_DELAY = 3000;

export interface ServiceInfo {
  id: string;
  category: string;
  name: string;
  status: "running" | "stopped" | "failed";
  status_detail?: string;
  uptime_secs: number;
  provider?: string;
  url?: string;
  kind?: string;
  workspace?: string;
  role?: "manager" | "worker";
}

export interface ServerMeta {
  started_at: number;
  port: number;
}

export interface ServicesSnapshot {
  server: ServerMeta;
  tunnel: ServiceInfo[];
  agents: ServiceInfo[];
  im_bots: ServiceInfo[];
  pty_session_count: number;
}

export function useServices() {
  const [data, setData] = useState<ServicesSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [connected, setConnected] = useState(false);
  const wsRef = useRef<WebSocket | null>(null);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // HTTP fallback fetch
  const fetchServices = useCallback(async () => {
    try {
      const res = await fetch(`${API_BASE}/api/services`);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const json: ServicesSnapshot = await res.json();
      setData(json);
      setError(null);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : "Unknown error");
    } finally {
      setLoading(false);
    }
  }, []);

  // Start HTTP polling fallback
  const startPolling = useCallback(() => {
    if (pollRef.current) return;
    fetchServices();
    pollRef.current = setInterval(fetchServices, POLL_INTERVAL);
  }, [fetchServices]);

  // Stop HTTP polling
  const stopPolling = useCallback(() => {
    if (pollRef.current) {
      clearInterval(pollRef.current);
      pollRef.current = null;
    }
  }, []);

  // WebSocket connection
  const connectWs = useCallback(() => {
    if (wsRef.current?.readyState === WebSocket.OPEN) return;

    const ws = new WebSocket(WS_URL);
    wsRef.current = ws;

    ws.onopen = () => {
      setConnected(true);
      setError(null);
      stopPolling();
    };

    ws.onmessage = (event) => {
      try {
        const snapshot: ServicesSnapshot = JSON.parse(event.data);
        setData(snapshot);
        setError(null);
        setLoading(false);
      } catch {
        // ignore parse errors
      }
    };

    ws.onclose = () => {
      setConnected(false);
      wsRef.current = null;
      // Fallback to polling, then try to reconnect WS
      startPolling();
      setTimeout(connectWs, WS_RECONNECT_DELAY);
    };

    ws.onerror = () => {
      // onclose will fire after this
    };
  }, [stopPolling, startPolling]);

  useEffect(() => {
    connectWs();
    return () => {
      stopPolling();
      if (wsRef.current) {
        wsRef.current.onclose = null; // prevent reconnect on unmount
        wsRef.current.close();
        wsRef.current = null;
      }
    };
  }, [connectWs, stopPolling]);

  const killService = useCallback(
    async (category: string, id: string) => {
      try {
        const res = await fetch(
          `${API_BASE}/api/services/${encodeURIComponent(category)}/${encodeURIComponent(id)}`,
          { method: "DELETE" }
        );
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        // If WS is connected, server will push the update.
        // If not, fetch manually.
        if (!connected) await fetchServices();
      } catch (e: unknown) {
        setError(e instanceof Error ? e.message : "Kill failed");
      }
    },
    [connected, fetchServices]
  );

  return { data, error, loading, connected, refresh: fetchServices, killService };
}
