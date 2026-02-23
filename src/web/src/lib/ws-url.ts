/**
 * WebSocket URL from current page host/protocol so it works on PC (localhost)
 * and on mobile via tunnel (same host, wss when page is https).
 */
export function getWebSocketUrl(path: string): string {
  if (typeof window === "undefined") return `ws://127.0.0.1:5182${path}`;
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  const host = window.location.host;
  return `${protocol}//${host}${path}`;
}
