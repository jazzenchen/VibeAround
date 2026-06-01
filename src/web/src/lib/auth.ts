/**
 * Session auth token plumbing for the web dashboard.
 *
 * The Tauri tray (or standalone launcher) opens the dashboard with a
 * `?token=<hex>` query parameter. On first load we:
 *
 *   1. Read the token from the URL
 *   2. Store it in `sessionStorage` so it survives in-app navigation
 *      but dies when the tab closes
 *   3. Strip the token from the address bar via `history.replaceState`
 *      so it never ends up in browser history or Referer headers
 *
 * Subsequent fetches add `Authorization: Bearer <token>` via the global
 * fetch wrapper in `main.tsx`. WebSocket URLs append `&token=<token>` via
 * `lib/ws-url.ts` since browsers can't set headers on WS handshakes.
 */

const STORAGE_KEY = "vibearound.auth.token";

export function initAuthFromUrl(): void {
  if (typeof window === "undefined") return;
  const params = new URLSearchParams(window.location.search);
  const token = params.get("token");
  if (!token) return;

  window.sessionStorage.setItem(STORAGE_KEY, token);

  // Strip ?token=... from the URL without reloading the page.
  params.delete("token");
  const query = params.toString();
  const newUrl =
    window.location.pathname + (query ? `?${query}` : "") + window.location.hash;
  window.history.replaceState(null, "", newUrl);
}

/** Return the currently cached auth token, if any. */
export function getAuthToken(): string | null {
  if (typeof window === "undefined") return null;
  return window.sessionStorage.getItem(STORAGE_KEY);
}
