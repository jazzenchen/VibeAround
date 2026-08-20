const THREAD_ID_PARAM = "thread_id";

export function webChatHandoffThreadId(rawUrl: string): string | undefined {
  const threadId = new URL(rawUrl).searchParams.get(THREAD_ID_PARAM)?.trim();
  return threadId || undefined;
}

export function urlWithoutWebChatHandoff(rawUrl: string): string {
  const url = new URL(rawUrl);
  url.searchParams.delete(THREAD_ID_PARAM);
  return `${url.pathname}${url.search}${url.hash}`;
}

export function readWebChatHandoffThreadId(): string | undefined {
  if (typeof window === "undefined") return undefined;
  return webChatHandoffThreadId(window.location.href);
}

export function clearWebChatHandoff(): void {
  if (typeof window === "undefined") return;
  window.history.replaceState(null, "", urlWithoutWebChatHandoff(window.location.href));
}
