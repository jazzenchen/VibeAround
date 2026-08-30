import { lazy, StrictMode, Suspense } from "react";
import { createRoot } from "react-dom/client";
import { I18nProvider } from "@va/i18n";
import App from "./App";
import { AppErrorBoundary } from "./components/AppErrorBoundary";
import { PairingGate } from "./PairingGate";
import { initTheme } from "./lib/theme";
import { clearAuthToken, getAuthToken, initAuthFromUrl } from "./lib/auth";
import { ownerPreviewSlug } from "./preview/previewRoute";
import "./index.css";

const PreviewPage = lazy(() => import("./preview/PreviewPage"));

initTheme();

// Pull ?token=... out of the URL on first load, stash it in sessionStorage,
// and strip the query string so it never ends up in history or Referer.
// Must run before any fetch or WebSocket is issued.
initAuthFromUrl();

// The fetch wrapper sets `vibearound.auth.reloading` before force-reloading
// the page on a 401, to prevent a reload loop when the daemon is actually
// unreachable. Clear it on every successful boot so a later stale-token
// detection can fire its own reload again.
sessionStorage.removeItem("vibearound.auth.reloading");

// Same-origin fetch wrapper:
//   - attaches `Authorization: Bearer <token>` to every API call
//   - sets the loca.lt bypass header so public tunnels don't show a
//     click-through interstitial
const BYPASS_HEADER = "bypass-tunnel-reminder";
const BYPASS_USER_AGENT = "VibeAround/1.0";
const originalFetch = window.fetch;
window.fetch = async function (input: RequestInfo | URL, init?: RequestInit) {
  const url =
    typeof input === "string"
      ? input
      : input instanceof URL
        ? input.href
        : input.url;
  const isSameOrigin =
    typeof url === "string" &&
    (url.startsWith("/") || url.startsWith(window.location.origin));
  const opts = { ...init };
  if (isSameOrigin) {
    const headers = new Headers(opts.headers);
    headers.set(BYPASS_HEADER, "1");
    if (!headers.has("User-Agent")) headers.set("User-Agent", BYPASS_USER_AGENT);
    // Token is only attached on same-origin calls — never leak it cross-origin.
    const token = getAuthToken();
    if (token && !headers.has("Authorization")) {
      headers.set("Authorization", `Bearer ${token}`);
    }
    opts.headers = headers;
  }
  const res = await originalFetch.call(this, input, opts);
  // A 401 invalidates the session credential.
  if (res.status === 401 && isSameOrigin) {
    const isApiCall =
      typeof url === "string" &&
      (url.includes("/api/") || url.includes("/mcp") || url.includes("/ws"));
    const hadBearerToken = getAuthToken() !== null;
    if (isApiCall && hadBearerToken) {
      clearAuthToken();
      // Reload once to re-evaluate the auth gate.
      if (!sessionStorage.getItem("vibearound.auth.reloading")) {
        sessionStorage.setItem("vibearound.auth.reloading", "1");
        window.location.reload();
      }
    }
  }
  return res;
};

// Render pairing when no session credential is available.
const hasToken = getAuthToken() !== null;
const previewSlug = ownerPreviewSlug(window.location.pathname);

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <I18nProvider>
      <AppErrorBoundary>
        {previewSlug ? (
          <Suspense
            fallback={
              <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
                Loading Preview…
              </div>
            }
          >
            <PreviewPage initialSlug={previewSlug} />
          </Suspense>
        ) : hasToken ? (
          <App />
        ) : (
          <PairingGate />
        )}
      </AppErrorBoundary>
    </I18nProvider>
  </StrictMode>
);
