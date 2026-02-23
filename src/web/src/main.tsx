import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./index.css";

// Bypass loca.lt tunnel reminder: add header or non-standard User-Agent on same-origin fetch.
// See: https://loca.lt — "Set bypass-tunnel-reminder request header with any value" or custom User-Agent.
const BYPASS_HEADER = "bypass-tunnel-reminder";
const BYPASS_USER_AGENT = "VibeAround/1.0";
const originalFetch = window.fetch;
window.fetch = function (input: RequestInfo | URL, init?: RequestInit) {
  const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
  const isSameOrigin =
    typeof url === "string" &&
    (url.startsWith("/") || url.startsWith(window.location.origin));
  const opts = { ...init };
  if (isSameOrigin && opts) {
    const headers = new Headers(opts.headers);
    headers.set(BYPASS_HEADER, "1");
    if (!headers.has("User-Agent")) headers.set("User-Agent", BYPASS_USER_AGENT);
    opts.headers = headers;
  }
  return originalFetch.call(this, input, opts);
};

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>
);
