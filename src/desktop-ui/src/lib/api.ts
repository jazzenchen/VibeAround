/**
 * Desktop-ui API client with bearer-token auth.
 *
 * The desktop-ui runs inside a Tauri webview whose origin is NOT the same
 * as the daemon's web server (`http://127.0.0.1:12358`), so all calls are
 * cross-origin and require a bearer token. The token is generated per
 * daemon start and exposed via the Tauri `get_auth_token` command.
 *
 * We cache the fetched token in module state for the life of the webview.
 * A restart of the daemon invalidates it — any 401 response triggers a
 * one-time refetch, so the UI recovers without the user having to reload.
 */

import { invoke } from "@tauri-apps/api/core";
import { loopbackBaseUrl, loopbackWsBaseUrl } from "@va/client";

export const DAEMON_PORT = 12358;
export const API_BASE = loopbackBaseUrl(DAEMON_PORT);
const WS_BASE = loopbackWsBaseUrl(DAEMON_PORT);

interface AuthFile {
  port: number;
  token: string;
}

let cachedToken: string | null = null;

async function fetchToken(): Promise<string | null> {
  try {
    const file = await invoke<AuthFile | null>("get_auth_token");
    if (file?.token) {
      cachedToken = file.token;
      return file.token;
    }
  } catch (e) {
    console.warn("[desktop-ui] get_auth_token failed:", e);
  }
  return null;
}

async function getToken(): Promise<string | null> {
  if (cachedToken) return cachedToken;
  return fetchToken();
}

export async function getLocalAgentApiToken(): Promise<string | null> {
  try {
    const file = await invoke<AuthFile | null>("get_local_agent_api_token");
    return file?.token ?? null;
  } catch (e) {
    console.warn("[desktop-ui] get_local_agent_api_token failed:", e);
    return null;
  }
}

/**
 * Authenticated fetch against the daemon. Transparently re-fetches the
 * token on a 401 (daemon restart invalidates the previous token).
 */
export async function apiFetch(path: string, init: RequestInit = {}): Promise<Response> {
  const doFetch = async (token: string | null): Promise<Response> => {
    const headers = new Headers(init.headers);
    if (token) headers.set("Authorization", `Bearer ${token}`);
    return fetch(`${API_BASE}${path}`, { ...init, headers });
  };

  let res = await doFetch(await getToken());
  if (res.status === 401) {
    cachedToken = null;
    const fresh = await fetchToken();
    if (fresh) res = await doFetch(fresh);
  }
  return res;
}

/**
 * Build an authenticated WebSocket URL with `?token=...` appended.
 * WebSockets cannot carry custom headers, so the daemon accepts the
 * token via query string.
 */
export async function authedWsUrl(path: string): Promise<string> {
  const token = await getToken();
  const url = `${WS_BASE}${path}`;
  if (!token) return url;
  const sep = url.includes("?") ? "&" : "?";
  return `${url}${sep}token=${encodeURIComponent(token)}`;
}

/**
 * Append the current auth token as a `?token=` query parameter to an HTTP
 * URL. Used when opening the dashboard (or a tunnel URL) in an external
 * browser — the SPA reads the token on first load, stores it in
 * sessionStorage, then strips the query via `history.replaceState`.
 *
 * Accepts both loopback URLs (`http://127.0.0.1:12358`) and arbitrary
 * tunnel URLs. The token is identical in both cases because the server
 * validates it regardless of which host it was reached on.
 */
async function authedDashboardUrl(url: string): Promise<string> {
  const token = await getToken();
  if (!token) return url;
  const sep = url.includes("?") ? "&" : "?";
  return `${url}${sep}token=${encodeURIComponent(token)}`;
}

/**
 * Open a daemon URL (loopback dashboard or tunnel) in the user's default
 * external browser, with the auth token automatically appended. Use this
 * instead of a raw `<a href>` anywhere the target is a daemon-backed page.
 *
 * Goes through the `open_external_url` Tauri command rather than
 * `window.open`, because `window.open` inside a Tauri webview creates a
 * child webview instead of handing off to the OS-level browser.
 */
export async function openDashboardUrl(url: string): Promise<void> {
  const withToken = await authedDashboardUrl(url);
  await openExternalUrl(withToken);
}

export async function openExternalUrl(url: string): Promise<void> {
  try {
    await invoke("open_external_url", { url });
  } catch (e) {
    console.warn("[desktop-ui] open_external_url failed, falling back:", e);
    window.open(url, "_blank", "noopener,noreferrer");
  }
}
