/**
 * PairingGate — browser pairing flow.
 *
 * Shown when the user opens the dashboard without a valid auth token.
 * Displays a 6-digit pairing code that the user sends via IM `/pair`
 * command. The page polls for verification and auto-reloads on success.
 *
 * Also keeps the legacy "paste token" fallback for advanced users.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { Check, Copy } from "lucide-react";
import { useI18n } from "@va/i18n";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { LanguageMenu } from "@/components/LanguageMenu";

const STORAGE_KEY = "vibearound.auth.token";
const POLL_INTERVAL_MS = 2000;
const CODE_TTL_SECS = 60;

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const textarea = document.createElement("textarea");
    textarea.value = text;
    textarea.setAttribute("readonly", "true");
    textarea.style.position = "fixed";
    textarea.style.opacity = "0";
    document.body.appendChild(textarea);
    textarea.select();
    document.execCommand("copy");
    document.body.removeChild(textarea);
  }
}

/**
 * Only allow same-origin redirects after pairing. Anything else (absolute
 * URL, protocol-relative, or scheme like `javascript:`) is rejected to
 * prevent open-redirect / phishing.
 */
function isSafeNext(next: string): boolean {
  return next.startsWith("/") && !next.startsWith("//");
}

type PairStatus = "loading" | "pending" | "expired" | "verified";

export function PairingGate() {
  const { t } = useI18n();
  const [code, setCode] = useState<string | null>(null);
  const [status, setStatus] = useState<PairStatus>("loading");
  const [remaining, setRemaining] = useState(CODE_TTL_SECS);
  const [showTokenInput, setShowTokenInput] = useState(false);
  const [pasted, setPasted] = useState("");
  const [tokenError, setTokenError] = useState<string | null>(null);
  const [pairCommandCopied, setPairCommandCopied] = useState(false);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const copyTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const startTimeRef = useRef<number>(0);
  const pairCommand = code ? `/pair ${code}` : "";

  const stopPolling = useCallback(() => {
    if (pollRef.current) {
      clearInterval(pollRef.current);
      pollRef.current = null;
    }
    if (timerRef.current) {
      clearInterval(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  const startPairing = useCallback(async () => {
    stopPolling();
    setStatus("loading");
    setCode(null);
    setRemaining(CODE_TTL_SECS);
    setPairCommandCopied(false);

    try {
      const res = await fetch("/va/api/pair/start", { method: "POST" });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data = await res.json();
      setCode(data.code);
      setStatus("pending");
      startTimeRef.current = Date.now();

      // Start countdown timer
      timerRef.current = setInterval(() => {
        const elapsed = Math.floor((Date.now() - startTimeRef.current) / 1000);
        const left = Math.max(0, CODE_TTL_SECS - elapsed);
        setRemaining(left);
        if (left <= 0) {
          setStatus("expired");
          stopPolling();
        }
      }, 1000);

      // Start polling for verification
      pollRef.current = setInterval(async () => {
        try {
          const r = await fetch(`/va/api/pair/status?sid=${encodeURIComponent(data.sid)}`);
          if (!r.ok) return;
          const d = await r.json();
          if (d.status === "verified") {
            stopPolling();
            setStatus("verified");
            // Store the token in sessionStorage for API/WS auth.
            // The cookie is also set (by the status endpoint) for /u preview routes.
            if (d.token) {
              window.sessionStorage.setItem(STORAGE_KEY, d.token);
            }
            // Honor ?next=<url> from the gate URL so the user lands on
            // the page they originally tried to open. Default to dashboard.
            const params = new URLSearchParams(window.location.search);
            const nextRaw = params.get("next");
            const next = nextRaw && isSafeNext(nextRaw) ? nextRaw : "/va/";
            setTimeout(() => {
              window.location.replace(next);
            }, 500);
          } else if (d.status === "expired") {
            stopPolling();
            setStatus("expired");
          }
        } catch {
          // Ignore transient network errors during polling.
        }
      }, POLL_INTERVAL_MS);
    } catch {
      setStatus("expired");
    }
  }, [stopPolling]);

  useEffect(() => {
    startPairing();
    return stopPolling;
  }, [startPairing, stopPolling]);

  useEffect(() => {
    return () => {
      if (copyTimerRef.current) {
        clearTimeout(copyTimerRef.current);
      }
    };
  }, []);

  const copyPairCommand = useCallback(async () => {
    if (!pairCommand) return;
    await copyText(pairCommand);
    setPairCommandCopied(true);
    if (copyTimerRef.current) {
      clearTimeout(copyTimerRef.current);
    }
    copyTimerRef.current = setTimeout(() => setPairCommandCopied(false), 1500);
  }, [pairCommand]);

  const submitToken = (e: React.FormEvent) => {
    e.preventDefault();
    const trimmed = pasted.trim();
    if (!trimmed) {
      setTokenError(t("Token is required."));
      return;
    }
    if (!/^[0-9a-fA-F]{32,}$/.test(trimmed)) {
      setTokenError(t("That doesn't look like a VibeAround auth token."));
      return;
    }
    window.sessionStorage.setItem(STORAGE_KEY, trimmed);
    window.location.reload();
  };

  return (
    <div className="flex min-h-screen items-center justify-center bg-background px-6 py-12 text-foreground">
      <div className="w-full max-w-md space-y-6">
        <div className="flex justify-end">
          <LanguageMenu />
        </div>
        {/* Header badge */}
        <div className="flex items-center gap-3">
          <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-primary/10 text-primary">
            <svg
              xmlns="http://www.w3.org/2000/svg"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth={2}
              strokeLinecap="round"
              strokeLinejoin="round"
              className="h-5 w-5"
              aria-hidden="true"
            >
              <path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71" />
              <path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71" />
            </svg>
          </div>
          <div>
            <h1 className="text-base font-semibold tracking-tight">
              {t("Pair your browser")}
            </h1>
            <p className="text-xs text-muted-foreground">
              {t("Connect this browser to your VibeAround instance.")}
            </p>
          </div>
        </div>

        {/* Pairing code display */}
        <div className="rounded-lg border border-border bg-card/50 p-6 text-center">
          {status === "loading" && (
            <p className="text-sm text-muted-foreground">{t("Generating pairing code…")}</p>
          )}

          {(status === "pending" || status === "expired") && code && (
            <>
              <p className="mb-3 text-xs font-medium uppercase tracking-wide text-muted-foreground">
                {t("Your pairing code")}
              </p>
              <p
                className={`font-mono text-4xl font-bold tracking-[0.3em] ${
                  status === "expired"
                    ? "text-muted-foreground/40 line-through"
                    : "text-foreground"
                }`}
              >
                {code}
              </p>
              <div className="mt-3 flex items-center justify-center gap-2 text-xs text-muted-foreground">
                {status === "pending" ? (
                  <>
                    <span className="inline-block h-2 w-2 animate-pulse rounded-full bg-primary" />
                    <span>
                      {t("Waiting for /pair {{code}} · {{seconds}}s", {
                        code,
                        seconds: remaining,
                      })}
                    </span>
                  </>
                ) : (
                  <span className="text-destructive">{t("Code expired")}</span>
                )}
              </div>
            </>
          )}

          {status === "verified" && (
            <p className="text-sm font-medium text-primary">
              {t("✓ Paired! Loading dashboard…")}
            </p>
          )}
        </div>

        {/* Instructions */}
        {status !== "verified" && (
          <div className="space-y-2">
            <p className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
              {t("How to pair")}
            </p>
            <ol className="list-decimal space-y-1 pl-5 text-sm text-foreground/90">
              <li>{t("Open any IM channel connected to VibeAround.")}</li>
              <li>
                <div className="inline-flex flex-wrap items-center gap-2">
                  <span>{t("Send")}</span>
                  <span className="rounded bg-muted px-1 py-0.5 font-mono text-xs">
                    {pairCommand || "/pair ···"}
                  </span>
                  <Button
                    type="button"
                    variant="outline"
                    size="xs"
                    disabled={!pairCommand}
                    onClick={copyPairCommand}
                    className="h-6 px-2 text-xs"
                  >
                    {pairCommandCopied ? (
                      <Check className="h-3 w-3" />
                    ) : (
                      <Copy className="h-3 w-3" />
                    )}
                    {pairCommandCopied ? t("Copied") : t("Copy")}
                  </Button>
                </div>
              </li>
              <li>{t("This page will update automatically.")}</li>
            </ol>
          </div>
        )}

        {/* Refresh button when expired */}
        {status === "expired" && (
          <Button
            type="button"
            onClick={startPairing}
            className="w-full"
          >
            {t("Generate new code")}
          </Button>
        )}

        {/* Token fallback */}
        {status !== "verified" && (
          <details
            className="group rounded-lg border border-border bg-card/50"
            open={showTokenInput}
            onToggle={(e) => setShowTokenInput((e.target as HTMLDetailsElement).open)}
          >
            <summary className="cursor-pointer select-none px-4 py-3 text-xs font-medium text-muted-foreground transition-colors hover:text-foreground">
              {t("I have a token — paste it")}
            </summary>
            <form onSubmit={submitToken} className="space-y-3 border-t border-border px-4 py-3">
              <label className="block space-y-1.5">
                <span className="text-xs text-muted-foreground">
                  {t("Session auth token (from ~/.vibearound/auth.json)")}
                </span>
                <Input
                  type="text"
                  autoComplete="off"
                  spellCheck={false}
                  value={pasted}
                  onChange={(e) => {
                    setPasted(e.target.value);
                    if (tokenError) setTokenError(null);
                  }}
                  placeholder={t("hex token…")}
                  className="font-mono text-xs"
                />
              </label>
              {tokenError && (
                <p className="text-xs text-destructive">{tokenError}</p>
              )}
              <Button
                type="submit"
                size="sm"
                className="w-full text-xs"
              >
                {t("Unlock dashboard")}
              </Button>
            </form>
          </details>
        )}

        {/* Footer */}
        <p className="text-center text-[10px] text-muted-foreground/60">
          {t("VibeAround · pairing codes expire after 1 minute")}
        </p>
      </div>
    </div>
  );
}
