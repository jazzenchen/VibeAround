import { useEffect, useState } from "react";
import { browserBaseUrl } from "@va/client";
import { getAuthToken } from "@/lib/auth";

/**
 * Measures round-trip latency every 10s for the header's
 * "connected · XX ms" indicator. With a session credential present it pings
 * an authenticated API route, so a daemon restart — which rotates the token —
 * trips the global 401 handler within one interval and sends the page back
 * through the pairing gate instead of leaving a half-dead dashboard that only
 * reacts once the user clicks something. Without a credential it falls back
 * to an unauthenticated HEAD `/`. Returns `null` on error.
 */
export function usePing(intervalMs = 10_000): number | null {
  const [pingMs, setPingMs] = useState<number | null>(null);

  useEffect(() => {
    if (typeof window === "undefined") return;
    const measure = async () => {
      const start = performance.now();
      try {
        const response = getAuthToken()
          ? await fetch(`${browserBaseUrl()}/api/service/info`, {
              cache: "no-store",
            })
          : await fetch(window.location.origin + "/", {
              method: "HEAD",
              cache: "no-store",
            });
        setPingMs(response.ok ? Math.round(performance.now() - start) : null);
      } catch {
        setPingMs(null);
      }
    };
    measure();
    const interval = setInterval(measure, intervalMs);
    return () => clearInterval(interval);
  }, [intervalMs]);

  return pingMs;
}
