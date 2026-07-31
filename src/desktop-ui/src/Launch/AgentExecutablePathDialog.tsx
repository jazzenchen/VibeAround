import { useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Check, RefreshCw } from "lucide-react";
import { useI18n } from "@va/i18n";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import type {
  AgentExecutableCandidate,
  AgentExecutableLatest,
  AgentExecutableResolution,
  AgentSummary,
} from "./api";
import type { AgentLaunchPreference } from "./types";
import { WindowsStartAppPickerDialog } from "./WindowsStartAppPickerDialog";

interface Props {
  agent: AgentSummary | null;
  preference?: AgentLaunchPreference;
  executableResolution?: AgentExecutableResolution | null;
  executableLoading?: boolean;
  fallbackExecutablePath?: string | null;
  busy: boolean;
  onClose: () => void;
  onSaveExecutablePath: (
    path: string | null,
    sourceLabel?: string | null,
  ) => Promise<void>;
  onRefreshExecutableResolution?: () => Promise<void>;
  onCheckLatest?: (path: string) => Promise<AgentExecutableLatest>;
  onUpdateAgent?: (path: string) => Promise<void>;
}

type ClientOs = "macos" | "windows" | "linux";
type LatestState = {
  signature: string;
  loading: boolean;
  latestVersion?: string | null;
  updateAvailable?: boolean | null;
  error?: string | null;
};

function detectClientOs(): ClientOs {
  const platform = (
    typeof navigator === "undefined" ? "" : navigator.platform || ""
  ).toLowerCase();
  const ua = (
    typeof navigator === "undefined" ? "" : navigator.userAgent || ""
  ).toLowerCase();
  const source = `${platform} ${ua}`;
  if (source.includes("win")) return "windows";
  if (source.includes("mac")) return "macos";
  return "linux";
}

function configuredPath(
  preference?: AgentLaunchPreference,
  resolution?: AgentExecutableResolution | null,
  fallbackPath = "",
): string {
  return (
    resolution?.configuredPath ??
    resolution?.selected?.path ??
    preference?.executable?.path ??
    preference?.executablePath ??
    fallbackPath
  );
}

function pathMatchesCandidate(
  path: string,
  candidate: AgentExecutableCandidate,
): boolean {
  const trimmed = path.trim();
  return (
    trimmed.length > 0 &&
    (trimmed === candidate.path || trimmed === (candidate.realpath ?? ""))
  );
}

function candidateLatestSignature(candidate: AgentExecutableCandidate): string {
  return `${candidate.version ?? ""}\u0000${candidate.updateCommand ?? ""}`;
}

function versionSummary(
  candidate: AgentExecutableCandidate,
  latestState: LatestState | undefined,
  checkingEnabled: boolean,
  t: (key: string, params?: Record<string, string | number>) => string,
): string | null {
  if (checkingEnabled && !latestState && candidate.updateCommand) {
    return t("Checking update");
  }
  if (latestState?.loading) return t("Checking update");
  const updateAvailable =
    latestState?.updateAvailable ?? candidate.updateAvailable;
  const latestVersion = latestState?.latestVersion ?? candidate.latestVersion;
  if (updateAvailable && latestVersion) {
    return t("New {{version}}", { version: latestVersion });
  }
  if (updateAvailable === false) {
    return t("Up to date");
  }
  if (latestVersion) {
    return t("Latest {{version}}", { version: latestVersion });
  }
  return null;
}

export function AgentExecutablePathDialog({
  agent,
  preference,
  executableResolution,
  executableLoading = false,
  fallbackExecutablePath,
  busy,
  onClose,
  onSaveExecutablePath,
  onRefreshExecutableResolution,
  onCheckLatest,
  onUpdateAgent,
}: Props) {
  const { t } = useI18n();
  const initialPath = useMemo(
    () =>
      configuredPath(
        preference,
        executableResolution,
        fallbackExecutablePath ?? "",
      ),
    [preference, executableResolution, fallbackExecutablePath],
  );
  const initialSourceLabel =
    preference?.executable?.source === "windows_start_apps_manual"
      ? preference.executable.sourceLabel
      : null;
  const [executablePath, setExecutablePath] = useState(initialPath);
  const [executableSourceLabel, setExecutableSourceLabel] = useState<
    string | null
  >(initialSourceLabel);
  const [startAppPickerOpen, setStartAppPickerOpen] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [updatingPath, setUpdatingPath] = useState<string | null>(null);
  const [latestByPath, setLatestByPath] = useState<Record<string, LatestState>>(
    {},
  );
  const executableCandidates = useMemo(
    () => executableResolution?.candidates ?? [],
    [executableResolution],
  );
  const candidateLatestKey = useMemo(
    () =>
      executableCandidates
        .map(
          (candidate) =>
            `${candidate.path}\u0000${candidateLatestSignature(candidate)}`,
        )
        .join("\u0001"),
    [executableCandidates],
  );
  const latestAgentId = agent?.id ?? "";
  const latestAgentDirectOnly = Boolean(agent?.direct_only);

  useEffect(() => {
    setExecutablePath(initialPath);
    setExecutableSourceLabel(initialSourceLabel);
    setStartAppPickerOpen(false);
    setSaveError(null);
    setSaving(false);
    setUpdatingPath(null);
    setLatestByPath({});
  }, [agent?.id, initialPath, initialSourceLabel]);

  useEffect(() => {
    if (!latestAgentId || latestAgentDirectOnly || !onCheckLatest) return;
    if (executableLoading) {
      setLatestByPath({});
      return;
    }
    if (!executableCandidates.length) {
      setLatestByPath({});
      return;
    }

    let cancelled = false;
    setLatestByPath((current) => {
      const next: Record<string, LatestState> = {};
      for (const candidate of executableCandidates) {
        const signature = candidateLatestSignature(candidate);
        const previous = current[candidate.path];
        next[candidate.path] = current[candidate.path] ?? {
          signature,
          loading: Boolean(candidate.updateCommand),
          latestVersion: candidate.latestVersion ?? null,
          updateAvailable: candidate.updateAvailable ?? null,
        };
        if (previous?.signature !== signature) {
          next[candidate.path] = {
            signature,
            loading: Boolean(candidate.updateCommand),
            latestVersion: candidate.latestVersion ?? null,
            updateAvailable: candidate.updateAvailable ?? null,
          };
        }
      }
      return next;
    });

    void (async () => {
      for (const candidate of executableCandidates) {
        if (!candidate.updateCommand) continue;
        if (cancelled) return;
        setLatestByPath((current) => ({
          ...current,
          [candidate.path]: {
            ...(current[candidate.path] ?? {}),
            signature: candidateLatestSignature(candidate),
            loading: true,
            error: null,
          },
        }));
        try {
          const latest = await onCheckLatest(candidate.path);
          if (cancelled) return;
          setLatestByPath((current) => ({
            ...current,
            [candidate.path]: {
              signature: candidateLatestSignature(candidate),
              loading: false,
              latestVersion: latest.latestVersion ?? null,
              updateAvailable: latest.updateAvailable ?? null,
            },
          }));
        } catch (error) {
          if (cancelled) return;
          setLatestByPath((current) => ({
            ...current,
            [candidate.path]: {
              ...(current[candidate.path] ?? {}),
              signature: candidateLatestSignature(candidate),
              loading: false,
              error: error instanceof Error ? error.message : String(error),
            },
          }));
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [
    candidateLatestKey,
    executableCandidates,
    executableLoading,
    latestAgentDirectOnly,
    latestAgentId,
    onCheckLatest,
  ]);

  if (!agent) return null;

  const clientOs = detectClientOs();
  const isDesktopApp = agent.direct_only;
  const isWindowsDesktopApp = isDesktopApp && clientOs === "windows";
  const executableDirty =
    executablePath.trim() !== initialPath ||
    executableSourceLabel !== initialSourceLabel;
  const dialogBusy = busy || saving;

  async function save() {
    if (saving) return;
    setSaveError(null);
    setSaving(true);
    try {
      await onSaveExecutablePath(
        executablePath.trim() || null,
        executableSourceLabel,
      );
      onClose();
    } catch (error) {
      setSaveError(error instanceof Error ? error.message : String(error));
      setSaving(false);
    }
  }

  async function chooseExecutable() {
    const chooseMacAppBundle = isDesktopApp && clientOs === "macos";
    const selected = await open({
      directory: chooseMacAppBundle,
      multiple: false,
      title: isDesktopApp
        ? t("Choose desktop app executable")
        : t("Choose agent executable"),
      filters:
        !chooseMacAppBundle && clientOs === "windows"
          ? [{ name: "Executable", extensions: ["exe"] }]
          : undefined,
    });
    const path = Array.isArray(selected) ? selected[0] : selected;
    if (path) {
      setExecutablePath(path);
      setExecutableSourceLabel(null);
    }
  }

  async function updateAgent(path: string) {
    if (!onUpdateAgent) return;
    setSaveError(null);
    setUpdatingPath(path);
    try {
      await onUpdateAgent(path);
      await onRefreshExecutableResolution?.();
    } catch (error) {
      setSaveError(error instanceof Error ? error.message : String(error));
    } finally {
      setUpdatingPath(null);
    }
  }

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="!flex max-h-[calc(100vh-64px)] w-[min(660px,calc(100vw-28px))] max-w-[calc(100vw-28px)] flex-col overflow-hidden p-0 sm:max-w-[min(660px,calc(100vw-28px))]">
        <DialogHeader className="shrink-0 border-b border-border px-5 py-3 pr-12">
          <DialogTitle className="text-lg">
            {isWindowsDesktopApp
              ? t("{{agent}} app launch target", {
                  agent: agent.display_name,
                })
              : isDesktopApp
                ? t("{{agent}} app path", { agent: agent.display_name })
                : t("{{agent}} launch path", { agent: agent.display_name })}
          </DialogTitle>
          <DialogDescription className="sr-only">
            {isWindowsDesktopApp
              ? t("Choose the desktop app launch target.")
              : isDesktopApp
                ? t("Choose the desktop app executable.")
                : t("Choose the CLI path used by Launch and ACP.")}
          </DialogDescription>
        </DialogHeader>

        <div className="px-5">
          <section className="space-y-2">
            <div className="text-xs text-muted-foreground">
              {isWindowsDesktopApp
                ? t("Choose the desktop app launch target.")
                : isDesktopApp
                  ? t("Choose the desktop app executable.")
                  : t("Choose the agent path.")}
            </div>

            {!isDesktopApp && (
              <div className="max-h-[220px] space-y-2 overflow-y-auto pr-1">
                {executableLoading ? (
                  <div className="px-1 py-2 text-xs text-muted-foreground">
                    <RefreshCw className="mr-1.5 inline h-3 w-3 animate-spin align-[-2px]" />
                    {t("Scanning local CLIs")}
                  </div>
                ) : executableCandidates.length ? (
                  executableCandidates.map((candidate) => {
                    const selected = pathMatchesCandidate(
                      executablePath,
                      candidate,
                    );
                    const candidateSignature =
                      candidateLatestSignature(candidate);
                    const latestState =
                      latestByPath[candidate.path]?.signature ===
                      candidateSignature
                        ? latestByPath[candidate.path]
                        : undefined;
                    const latest = versionSummary(
                      candidate,
                      latestState,
                      Boolean(onCheckLatest),
                      t,
                    );
                    const updating = updatingPath === candidate.path;
                    const updateAvailable =
                      latestState?.updateAvailable ?? candidate.updateAvailable;
                    const checkingLatest =
                      (Boolean(onCheckLatest) &&
                        !latestState &&
                        Boolean(candidate.updateCommand)) ||
                      Boolean(latestState?.loading);
                    const canUpdate =
                      Boolean(onUpdateAgent) &&
                      Boolean(candidate.updateCommand) &&
                      !checkingLatest &&
                      updateAvailable !== false;
                    return (
                      <div
                        key={`${candidate.source}:${candidate.path}`}
                        className={`flex w-full min-w-0 items-center gap-2 rounded-md border px-2.5 py-2 text-left transition-colors ${
                          selected
                            ? "border-primary/45 bg-primary/10"
                            : "border-border bg-card hover:border-primary/30"
                        }`}
                      >
                        <button
                          type="button"
                          disabled={dialogBusy}
                          className="flex min-w-0 flex-1 items-center gap-2 text-left"
                          onClick={() => setExecutablePath(candidate.path)}
                        >
                          <span className="flex h-5 w-5 shrink-0 items-center justify-center text-primary">
                            {selected ? (
                              <Check className="h-3.5 w-3.5" />
                            ) : (
                              <span className="h-2 w-2 rounded-full border border-muted-foreground/50" />
                            )}
                          </span>
                          <span className="min-w-0 flex-1">
                            <span className="block truncate font-mono text-[11px] [font-variant-ligatures:none]">
                              {candidate.path}
                            </span>
                            <span className="block truncate text-[10px] text-muted-foreground">
                              {candidate.sourceLabel}
                              {candidate.version
                                ? ` · ${candidate.version}`
                                : ""}
                            </span>
                          </span>
                        </button>
                        {latest && (
                          <span
                            className={`shrink-0 rounded-md border px-1.5 py-0.5 text-[10px] ${
                              updateAvailable
                                ? "border-primary/35 bg-primary/10 text-primary"
                                : "border-border bg-muted/40 text-muted-foreground"
                            }`}
                            title={latestState?.error ?? undefined}
                          >
                            {checkingLatest && (
                              <RefreshCw className="mr-1 inline h-3 w-3 animate-spin" />
                            )}
                            {latest}
                          </span>
                        )}
                        {canUpdate && (
                          <Button
                            type="button"
                            variant="ghost"
                            size="xs"
                            disabled={dialogBusy || Boolean(updatingPath)}
                            className="h-7 shrink-0 px-1.5 text-xs text-primary hover:bg-transparent hover:text-primary hover:underline"
                            title={
                              candidate.updateCommand ?? t("No update command")
                            }
                            onClick={() => void updateAgent(candidate.path)}
                          >
                            <RefreshCw
                              className={`h-3.5 w-3.5 ${updating ? "animate-spin" : ""}`}
                            />
                            {updating ? t("Updating") : t("Update")}
                          </Button>
                        )}
                      </div>
                    );
                  })
                ) : (
                  <div className="text-[11px] text-muted-foreground">
                    {t("No executable candidates found")}
                  </div>
                )}
              </div>
            )}

            {isWindowsDesktopApp && (
              <div className="flex flex-wrap items-center gap-2 pt-1">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={dialogBusy}
                  className="h-8 px-2.5 text-xs"
                  onClick={() => setStartAppPickerOpen(true)}
                >
                  {t("Choose installed app")}
                </Button>
                {executableSourceLabel && (
                  <span className="inline-flex min-w-0 items-center gap-1.5 text-xs text-primary">
                    <Check className="h-3.5 w-3.5 shrink-0" />
                    <span className="truncate">{executableSourceLabel}</span>
                  </span>
                )}
              </div>
            )}

            <div className="space-y-1 pt-1">
              <div className="text-[10px] font-medium text-muted-foreground">
                {isWindowsDesktopApp
                  ? t("Current launch target")
                  : t("Current selected path")}
              </div>
              <div className="flex gap-1.5">
                <Input
                  value={executablePath}
                  disabled={dialogBusy}
                  placeholder={
                    isWindowsDesktopApp
                      ? "OpenAI.Codex_...!App or C:\\Path\\To\\Codex.exe"
                      : clientOs === "windows"
                        ? "C:\\Path\\To\\Agent.exe"
                      : isDesktopApp
                        ? "/Applications/App.app"
                        : "/opt/homebrew/bin/agent"
                  }
                  className="!h-8 min-h-8 max-h-8 font-mono !text-[11px] leading-4 placeholder:!text-[11px] md:!text-[11px] [font-variant-ligatures:none]"
                  onChange={(event) => {
                    setExecutablePath(event.target.value);
                    setExecutableSourceLabel(null);
                  }}
                />
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={dialogBusy}
                  className="h-8 px-2.5 text-xs"
                  onClick={() => void chooseExecutable()}
                >
                  {isWindowsDesktopApp ? t("Browse .exe") : t("Choose")}
                </Button>
              </div>
            </div>
          </section>

          {saveError && (
            <div className="mt-2 shrink-0 text-xs text-destructive">
              {saveError}
            </div>
          )}
        </div>

        <DialogFooter className="shrink-0 !flex-row items-center justify-between border-t border-border px-5 py-3 sm:justify-between">
          <div>
            {!isDesktopApp && (
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={dialogBusy || executableLoading}
                className="h-8 px-2.5 text-xs"
                onClick={() => void onRefreshExecutableResolution?.()}
              >
                <RefreshCw className="h-3.5 w-3.5" />
                {t("Scan")}
              </Button>
            )}
          </div>
          <div className="ml-auto flex items-center gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={dialogBusy}
              onClick={onClose}
            >
              {t("Cancel")}
            </Button>
            <Button
              type="button"
              size="sm"
              disabled={dialogBusy || !executableDirty}
              onClick={() => void save()}
            >
              {t("Save")}
            </Button>
          </div>
        </DialogFooter>
        {isWindowsDesktopApp && (
          <WindowsStartAppPickerDialog
            open={startAppPickerOpen}
            agentId={agent.id}
            agentName={agent.display_name}
            selectedAppId={executablePath}
            onOpenChange={setStartAppPickerOpen}
            onSelect={(app) => {
              setExecutablePath(app.appId);
              setExecutableSourceLabel(app.name);
              setStartAppPickerOpen(false);
            }}
          />
        )}
      </DialogContent>
    </Dialog>
  );
}
