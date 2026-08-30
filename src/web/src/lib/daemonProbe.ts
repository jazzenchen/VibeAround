/**
 * One cheap authenticated request against the daemon, used to classify a
 * dropped connection. The global fetch wrapper in `main.tsx` does the rest:
 * a 401 (daemon restarted, token rotated) clears the session credential and
 * reloads into the pairing gate; a network error means the daemon is down
 * and the reconnect loops keep waiting. Browsers hide the close code of a
 * failed WebSocket upgrade, so this probe is the only way to tell the two
 * apart.
 */

import { browserBaseUrl } from "@va/client";
import { getAuthToken } from "./auth";

const PROBE_THROTTLE_MS = 3_000;

let lastProbeAt = 0;

export function probeDaemonAuth(): void {
  if (typeof window === "undefined") return;
  if (getAuthToken() === null) return;
  const now = Date.now();
  if (now - lastProbeAt < PROBE_THROTTLE_MS) return;
  lastProbeAt = now;
  void fetch(`${browserBaseUrl()}/api/service/info`, { cache: "no-store" }).catch(
    () => {
      // Network failure: the daemon is gone, not the credential. The socket
      // loops keep retrying and the header indicator shows offline.
    },
  );
}
