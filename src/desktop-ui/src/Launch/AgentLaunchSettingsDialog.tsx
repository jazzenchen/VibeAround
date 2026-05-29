import { useEffect, useMemo, useState } from "react";
import { Check, Plus, RotateCcw, X } from "lucide-react";
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
import {
  applyCodexSandboxPreset,
  inferCodexSandboxPreset,
  parseLaunchArgInput,
  sameArgs,
  type CodexSandboxPreset,
  type LaunchArgParseError,
} from "./agentLaunchArgs";
import type { AgentSummary } from "./api";
import type { AgentLaunchArgs, AgentLaunchPreference } from "./types";

interface Props {
  agent: AgentSummary | null;
  preference?: AgentLaunchPreference;
  busy: boolean;
  onClose: () => void;
  onSave: (launchArgs: AgentLaunchArgs) => Promise<void>;
}

const CODEX_SANDBOX_PRESETS: Array<{
  id: CodexSandboxPreset;
  label: string;
}> = [
  { id: "default", label: "Default" },
  { id: "read-only", label: "Read only" },
  { id: "workspace-write", label: "Workspace write" },
  { id: "danger-full-access", label: "Full access" },
];

function launchArgsFromPreference(
  preference?: AgentLaunchPreference,
): AgentLaunchArgs {
  return {
    terminal: [...(preference?.launchArgs?.terminal ?? [])],
    acp: [...(preference?.launchArgs?.acp ?? [])],
  };
}

export function AgentLaunchSettingsDialog({
  agent,
  preference,
  busy,
  onClose,
  onSave,
}: Props) {
  const { t } = useI18n();
  const initialArgs = useMemo(
    () => launchArgsFromPreference(preference),
    [preference],
  );
  const [terminalArgs, setTerminalArgs] = useState<string[]>(initialArgs.terminal ?? []);
  const [draftArg, setDraftArg] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setTerminalArgs(initialArgs.terminal ?? []);
    setDraftArg("");
    setError(null);
  }, [agent?.id, initialArgs]);

  if (!agent) return null;

  const sandboxPreset =
    agent.id === "codex" ? inferCodexSandboxPreset(terminalArgs) : null;
  const dirty = !sameArgs(terminalArgs, initialArgs.terminal ?? []);

  function parseErrorMessage(error: LaunchArgParseError): string {
    switch (error) {
      case "danglingEscape":
        return t("Backslash must escape a character");
      case "lineBreak":
        return t("Arguments cannot contain line breaks");
      case "unterminatedQuote":
        return t("Quote is not closed");
    }
  }

  function addArg() {
    const parsed = parseLaunchArgInput(draftArg);
    if (parsed.error) {
      setError(parseErrorMessage(parsed.error));
      return;
    }
    if (parsed.args.length === 0) return;
    setTerminalArgs((current) => [...current, ...parsed.args]);
    setDraftArg("");
    setError(null);
  }

  async function save() {
    setError(null);
    try {
      await onSave({
        ...initialArgs,
        terminal: terminalArgs,
      });
      onClose();
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    }
  }

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="!flex max-h-[calc(100vh-64px)] w-[min(680px,calc(100vw-32px))] max-w-[calc(100vw-32px)] flex-col overflow-hidden p-0 sm:max-w-[min(680px,calc(100vw-32px))]">
        <DialogHeader className="shrink-0 border-b border-border px-6 py-4 pr-12">
          <DialogTitle>
            {t("{{agent}} launch settings", { agent: agent.display_name })}
          </DialogTitle>
          <DialogDescription className="sr-only">
            {t("Configure per-agent launch arguments.")}
          </DialogDescription>
        </DialogHeader>

        <div className="min-h-0 flex-1 space-y-5 overflow-y-auto px-6 py-5">
          {agent.id === "codex" && sandboxPreset && (
            <section className="space-y-2">
              <div className="text-[11px] font-semibold uppercase tracking-[0.16em] text-muted-foreground/70">
                {t("Sandbox")}
              </div>
              <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
                {CODEX_SANDBOX_PRESETS.map((preset) => {
                  const active = sandboxPreset === preset.id;
                  return (
                    <Button
                      key={preset.id}
                      type="button"
                      variant={active ? "default" : "outline"}
                      size="sm"
                      disabled={busy}
                      className="min-w-0 justify-center px-2 text-xs"
                      onClick={() =>
                        setTerminalArgs((current) =>
                          applyCodexSandboxPreset(current, preset.id),
                        )
                      }
                    >
                      {active && <Check className="h-3 w-3" />}
                      {t(preset.label)}
                    </Button>
                  );
                })}
              </div>
            </section>
          )}

          <section className="space-y-2">
            <div className="flex items-center justify-between gap-3">
              <div className="text-[11px] font-semibold uppercase tracking-[0.16em] text-muted-foreground/70">
                {t("Terminal arguments")}
              </div>
              <Button
                type="button"
                variant="ghost"
                size="xs"
                disabled={busy || terminalArgs.length === 0}
                onClick={() => setTerminalArgs([])}
              >
                <RotateCcw className="h-3 w-3" />
                {t("Reset")}
              </Button>
            </div>

            <div className="min-h-[92px] rounded-md border border-border bg-background p-2">
              {terminalArgs.length > 0 ? (
                <div className="flex flex-wrap gap-1.5">
                  {terminalArgs.map((arg, index) => (
                    <span
                      key={`${arg}-${index}`}
                      className="inline-flex max-w-full min-w-0 items-center gap-1 rounded-md border border-border bg-muted/60 px-2 py-1 font-mono text-[11px]"
                    >
                      <span className="truncate">{arg}</span>
                      <button
                        type="button"
                        disabled={busy}
                        aria-label={t("Remove")}
                        title={t("Remove")}
                        className="rounded-sm text-muted-foreground hover:text-foreground disabled:pointer-events-none disabled:opacity-50"
                        onClick={() =>
                          setTerminalArgs((current) =>
                            current.filter((_, itemIndex) => itemIndex !== index),
                          )
                        }
                      >
                        <X className="h-3 w-3" />
                      </button>
                    </span>
                  ))}
                </div>
              ) : (
                <div className="flex h-[74px] items-center justify-center text-xs text-muted-foreground">
                  {t("No custom arguments")}
                </div>
              )}
            </div>

            <div className="flex gap-2">
              <Input
                value={draftArg}
                disabled={busy}
                placeholder={t("Argument or command-line fragment")}
                className="font-mono text-sm"
                onChange={(event) => setDraftArg(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    addArg();
                  }
                }}
              />
              <Button
                type="button"
                variant="outline"
                disabled={busy || !draftArg.trim()}
                onClick={addArg}
              >
                <Plus className="h-4 w-4" />
                {t("Add")}
              </Button>
            </div>

            {error && <div className="text-xs text-destructive">{error}</div>}
          </section>
        </div>

        <DialogFooter className="shrink-0 border-t border-border px-6 py-4">
          <Button type="button" variant="outline" disabled={busy} onClick={onClose}>
            {t("Cancel")}
          </Button>
          <Button type="button" disabled={busy || !dirty} onClick={() => void save()}>
            {t("Save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
