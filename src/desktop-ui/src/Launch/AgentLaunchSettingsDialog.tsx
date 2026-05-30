import { useEffect, useMemo, useState, type ReactNode } from "react";
import { Plus, RotateCcw, X } from "lucide-react";
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
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  parseLaunchArgInput,
  sameArgs,
  type LaunchArgParseError,
} from "./agentLaunchArgs";
import type { AgentSummary } from "./api";
import type { AgentLaunchArgs, AgentLaunchPreference } from "./types";

interface Props {
  agent: AgentSummary | null;
  preference?: AgentLaunchPreference;
  workspacePath?: string | null;
  windowLabel?: string | null;
  busy: boolean;
  onClose: () => void;
  onSave: (launchArgs: AgentLaunchArgs) => Promise<void>;
}

interface ArgsEditorProps {
  label: string;
  previewKind: LaunchArgTab;
  commandWords: string[];
  clientOs: ClientOs;
  workspacePath: string;
  windowLabel: string;
  args: string[];
  draftArg: string;
  busy: boolean;
  error: string | null;
  onChangeArgs: (args: string[]) => void;
  onChangeDraftArg: (arg: string) => void;
  onError: (error: string | null) => void;
  parseErrorMessage: (error: LaunchArgParseError) => string;
}

type LaunchArgTab = "terminal" | "acp";
type ClientOs = "macos" | "windows" | "linux";

const ACP_NPM_BIN_DIR = "~/.vibearound/plugins/node_modules/.bin";
const ACP_NPM_BIN_DIR_WINDOWS =
  "%USERPROFILE%\\.vibearound\\plugins\\node_modules\\.bin";

function launchArgsFromPreference(
  preference?: AgentLaunchPreference,
): AgentLaunchArgs {
  return {
    terminal: [...(preference?.launchArgs?.terminal ?? [])],
    acp: [...(preference?.launchArgs?.acp ?? [])],
  };
}

function commandWords(command: string): string[] {
  const parsed = parseLaunchArgInput(command);
  if (parsed.error || parsed.args.length === 0) return [command].filter(Boolean);
  return parsed.args;
}

function terminalCommandWords(agent: AgentSummary): string[] {
  return commandWords(agent.pty_command ?? agent.id);
}

function acpCommandWords(agent: AgentSummary, os: ClientOs): string[] {
  if (agent.acp_npm_package) {
    const binName =
      agent.acp_bin_name ?? npmPackageBinName(agent.acp_npm_package);
    const entry =
      os === "windows"
        ? `${ACP_NPM_BIN_DIR_WINDOWS}\\${binName}.cmd`
        : `${ACP_NPM_BIN_DIR}/${binName}`;
    return ["node", entry];
  }
  return [
    agent.acp_program ?? agent.id,
    ...(agent.acp_args ?? []),
  ].filter((word) => word.trim() !== "");
}

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

function npmPackageBinName(npmPackage: string): string {
  const source = npmPackage.trim();
  const versionIndex = source.startsWith("@")
    ? source.lastIndexOf("@")
    : source.indexOf("@");
  const packageName =
    versionIndex > 0 ? source.slice(0, versionIndex) : source;
  const segments = packageName.split("/");
  return segments[segments.length - 1] || packageName;
}

function quoteCommandWord(word: string): string {
  if (/^[A-Za-z0-9_@%+=:,./~-]+$/.test(word)) return word;
  return `'${word.replace(/'/g, `'\\''`)}'`;
}

function powershellSingleQuoted(value: string): string {
  return `'${value.replace(/'/g, "''")}'`;
}

export function AgentLaunchSettingsDialog({
  agent,
  preference,
  workspacePath,
  windowLabel,
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
  const [terminalDraftArg, setTerminalDraftArg] = useState("");
  const [terminalError, setTerminalError] = useState<string | null>(null);
  const [acpArgs, setAcpArgs] = useState<string[]>(initialArgs.acp ?? []);
  const [acpDraftArg, setAcpDraftArg] = useState("");
  const [acpError, setAcpError] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<LaunchArgTab>("terminal");

  useEffect(() => {
    setTerminalArgs(initialArgs.terminal ?? []);
    setTerminalDraftArg("");
    setTerminalError(null);
    setAcpArgs(initialArgs.acp ?? []);
    setAcpDraftArg("");
    setAcpError(null);
    setSaveError(null);
    setActiveTab("terminal");
  }, [agent?.id, initialArgs]);

  if (!agent) return null;

  const previewWorkspace = workspacePath?.trim() || "~";
  const previewWindowLabel =
    windowLabel?.trim() || `${agent.display_name} (direct)`;
  const clientOs = detectClientOs();
  const dirty =
    !sameArgs(terminalArgs, initialArgs.terminal ?? []) ||
    !sameArgs(acpArgs, initialArgs.acp ?? []);

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

  async function save() {
    setSaveError(null);
    try {
      await onSave({
        ...initialArgs,
        terminal: terminalArgs,
        acp: acpArgs,
      });
      onClose();
    } catch (error) {
      setSaveError(error instanceof Error ? error.message : String(error));
    }
  }

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="!flex h-[520px] max-h-[calc(100vh-64px)] w-[min(660px,calc(100vw-28px))] max-w-[calc(100vw-28px)] flex-col overflow-hidden p-0 sm:max-w-[min(660px,calc(100vw-28px))]">
        <DialogHeader className="shrink-0 border-b border-border px-5 py-3 pr-12">
          <DialogTitle className="text-lg">
            {t("{{agent}} launch settings", { agent: agent.display_name })}
          </DialogTitle>
          <DialogDescription className="sr-only">
            {t("Configure per-agent launch arguments.")}
          </DialogDescription>
        </DialogHeader>

        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-3">
          <Tabs
            value={activeTab}
            onValueChange={(value) => setActiveTab(value as LaunchArgTab)}
            className="gap-3"
          >
            <TabsList className="grid h-8 w-full grid-cols-2 rounded-md p-0.5">
              <TabsTrigger value="terminal" className="h-7 text-xs">
                {t("Terminal")}
              </TabsTrigger>
              <TabsTrigger value="acp" className="h-7 text-xs">
                {t("Agent protocol")}
              </TabsTrigger>
            </TabsList>

            <TabsContent value="terminal" className="mt-0 space-y-3">
              <ArgsEditor
                label={t("Terminal arguments")}
                previewKind="terminal"
                commandWords={terminalCommandWords(agent)}
                clientOs={clientOs}
                workspacePath={previewWorkspace}
                windowLabel={previewWindowLabel}
                args={terminalArgs}
                draftArg={terminalDraftArg}
                busy={busy}
                error={terminalError}
                onChangeArgs={setTerminalArgs}
                onChangeDraftArg={setTerminalDraftArg}
                onError={setTerminalError}
                parseErrorMessage={parseErrorMessage}
              />
            </TabsContent>

            <TabsContent value="acp" className="mt-0">
              <ArgsEditor
                label={t("Agent protocol arguments")}
                previewKind="acp"
                commandWords={acpCommandWords(agent, clientOs)}
                clientOs={clientOs}
                workspacePath={previewWorkspace}
                windowLabel={previewWindowLabel}
                args={acpArgs}
                draftArg={acpDraftArg}
                busy={busy}
                error={acpError}
                onChangeArgs={setAcpArgs}
                onChangeDraftArg={setAcpDraftArg}
                onError={setAcpError}
                parseErrorMessage={parseErrorMessage}
              />
            </TabsContent>
          </Tabs>

          {saveError && <div className="mt-3 text-xs text-destructive">{saveError}</div>}
        </div>

        <DialogFooter className="shrink-0 border-t border-border px-5 py-3">
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={busy}
            onClick={onClose}
          >
            {t("Cancel")}
          </Button>
          <Button
            type="button"
            size="sm"
            disabled={busy || !dirty}
            onClick={() => void save()}
          >
            {t("Save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function ArgsEditor({
  label,
  previewKind,
  commandWords,
  clientOs,
  workspacePath,
  windowLabel,
  args,
  draftArg,
  busy,
  error,
  onChangeArgs,
  onChangeDraftArg,
  onError,
  parseErrorMessage,
}: ArgsEditorProps) {
  const { t } = useI18n();

  function addArg() {
    const parsed = parseLaunchArgInput(draftArg);
    if (parsed.error) {
      onError(parseErrorMessage(parsed.error));
      return;
    }
    if (parsed.args.length === 0) return;
    onChangeArgs([...args, ...parsed.args]);
    onChangeDraftArg("");
    onError(null);
  }

  return (
    <section className="space-y-2">
      <div className="flex items-center justify-between gap-3">
        <div>
          <div className="text-[10px] font-semibold uppercase tracking-[0.16em] text-muted-foreground/70">
            {previewKind === "terminal" ? t("Launch script") : t("ACP command")}
          </div>
          <div className="mt-0.5 text-xs text-muted-foreground">{label}</div>
        </div>
        <Button
          type="button"
          variant="ghost"
          size="xs"
          disabled={busy || args.length === 0}
          onClick={() => onChangeArgs([])}
        >
          <RotateCcw className="h-3 w-3" />
          {t("Reset")}
        </Button>
      </div>

      <LaunchPreview
        kind={previewKind}
        commandWords={commandWords}
        clientOs={clientOs}
        workspacePath={workspacePath}
        windowLabel={windowLabel}
        customArgs={args}
        busy={busy}
        insertionLabel={t("Custom arguments")}
        removeLabel={t("Remove")}
        onRemoveArg={(index) =>
          onChangeArgs(args.filter((_, itemIndex) => itemIndex !== index))
        }
      />

      <div className="flex gap-1.5">
        <Input
          value={draftArg}
          disabled={busy}
          placeholder={t("Argument or command-line fragment")}
          className="!h-8 min-h-8 max-h-8 font-mono !text-[11px] leading-4 placeholder:!text-[11px] md:!text-[11px] [font-variant-ligatures:none]"
          onChange={(event) => onChangeDraftArg(event.target.value)}
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
          size="sm"
          disabled={busy || !draftArg.trim()}
          className="h-8 px-2.5 text-xs"
          onClick={addArg}
        >
          <Plus className="h-3.5 w-3.5" />
          {t("Add")}
        </Button>
      </div>

      {error && <div className="text-xs text-destructive">{error}</div>}
    </section>
  );
}

function LaunchPreview({
  kind,
  commandWords,
  clientOs,
  workspacePath,
  windowLabel,
  customArgs,
  busy,
  insertionLabel,
  removeLabel,
  onRemoveArg,
}: {
  kind: LaunchArgTab;
  commandWords: string[];
  clientOs: ClientOs;
  workspacePath: string;
  windowLabel: string;
  customArgs: string[];
  busy: boolean;
  insertionLabel: string;
  removeLabel: string;
  onRemoveArg: (index: number) => void;
}) {
  const baseWords = commandWords.length > 0 ? commandWords : ["agent"];

  if (kind === "acp") {
    return (
      <AcpProcessPreview
        commandWords={baseWords}
        clientOs={clientOs}
        workspacePath={workspacePath}
        customArgs={customArgs}
        busy={busy}
        insertionLabel={insertionLabel}
        removeLabel={removeLabel}
        onRemoveArg={onRemoveArg}
      />
    );
  }

  if (clientOs === "windows") {
    return (
      <WindowsLaunchPreview
        commandWords={baseWords}
        workspacePath={workspacePath}
        windowLabel={windowLabel}
        customArgs={customArgs}
        busy={busy}
        insertionLabel={insertionLabel}
        removeLabel={removeLabel}
        onRemoveArg={onRemoveArg}
      />
    );
  }

  return (
    <div className="rounded-md border border-border bg-muted/20 px-3 py-2 font-mono text-[11px] leading-5 [font-variant-ligatures:none]">
      <PreviewLine muted>#!/bin/bash</PreviewLine>
      <PreviewLine muted>rm -- "$0"</PreviewLine>
      <PreviewLine muted>set -e</PreviewLine>
      <PreviewLine muted>
        echo "# VibeAround profile: {windowLabel.replace(/"/g, "'")}"
      </PreviewLine>
      <PreviewLine muted>unset NO_COLOR</PreviewLine>
      <PreviewLine muted>
        if [ -z "${"{TERM:-}"}" ] || [ "$TERM" = "dumb" ]; then export
        TERM=xterm-256color; fi
      </PreviewLine>
      <PreviewLine muted>export COLORTERM=${"{COLORTERM:-truecolor}"}</PreviewLine>
      <PreviewLine muted>export CLICOLOR=${"{CLICOLOR:-1}"}</PreviewLine>
      <PreviewLine muted>cd {quoteCommandWord(workspacePath)}</PreviewLine>
      <div className="flex min-h-6 flex-wrap items-center gap-x-1.5 gap-y-1">
        <span className="text-muted-foreground">exec</span>
        <CommandTokens words={baseWords} quote={quoteCommandWord} />
        <CustomArgTokens
          customArgs={customArgs}
          quote={quoteCommandWord}
          busy={busy}
          insertionLabel={insertionLabel}
          removeLabel={removeLabel}
          onRemoveArg={onRemoveArg}
        />
      </div>
    </div>
  );
}

function WindowsLaunchPreview({
  commandWords,
  workspacePath,
  windowLabel,
  customArgs,
  busy,
  insertionLabel,
  removeLabel,
  onRemoveArg,
}: {
  commandWords: string[];
  workspacePath: string;
  windowLabel: string;
  customArgs: string[];
  busy: boolean;
  insertionLabel: string;
  removeLabel: string;
  onRemoveArg: (index: number) => void;
}) {
  const [program, ...programArgs] = commandWords;
  return (
    <div className="rounded-md border border-border bg-muted/20 px-3 py-2 font-mono text-[11px] leading-5 [font-variant-ligatures:none]">
      <PreviewLine muted>
        $Host.UI.RawUI.WindowTitle ={" "}
        {powershellSingleQuoted(`VibeAround - ${windowLabel}`)}
      </PreviewLine>
      <PreviewLine muted>
        Write-Host {powershellSingleQuoted(`# VibeAround profile: ${windowLabel}`)}
      </PreviewLine>
      <PreviewLine muted>Remove-Item Env:NO_COLOR -ErrorAction SilentlyContinue</PreviewLine>
      <PreviewLine muted>
        if (-not $env:TERM -or $env:TERM -eq 'dumb') {"{ "}
        $env:TERM = 'xterm-256color' {"}"}
      </PreviewLine>
      <PreviewLine muted>
        if (-not $env:COLORTERM) {"{ "} $env:COLORTERM = 'truecolor' {"}"}
      </PreviewLine>
      <PreviewLine muted>
        if (-not $env:CLICOLOR) {"{ "} $env:CLICOLOR = '1' {"}"}
      </PreviewLine>
      <PreviewLine muted>
        Set-Location -LiteralPath {powershellSingleQuoted(workspacePath)}
      </PreviewLine>
      <PreviewLine muted>
        $vaCommand = {powershellSingleQuoted(program ?? "agent")}
      </PreviewLine>
      <PreviewLine muted>$vaArgs = @(</PreviewLine>
      {programArgs.map((arg, index) => (
        <PreviewLine key={`base-ps-${arg}-${index}`}>
          {"  "}
          {powershellSingleQuoted(arg)}
        </PreviewLine>
      ))}
      {customArgs.length > 0 ? (
        customArgs.map((arg, index) => (
          <PreviewLine key={`custom-ps-${arg}-${index}`}>
            {"  "}
            <CustomArgToken
              arg={arg}
              index={index}
              quote={powershellSingleQuoted}
              busy={busy}
              removeLabel={removeLabel}
              onRemoveArg={onRemoveArg}
            />
          </PreviewLine>
        ))
      ) : (
        <PreviewLine>
          {"  "}
          <InsertionToken label={insertionLabel} />
        </PreviewLine>
      )}
      <PreviewLine muted>)</PreviewLine>
      <PreviewLine muted>& $vaCommand @vaArgs</PreviewLine>
      <PreviewLine muted>if ($LASTEXITCODE -ne $null -and $LASTEXITCODE -ne 0) {"{"}</PreviewLine>
      <PreviewLine muted>  Write-Host "`nCommand exited with code $LASTEXITCODE"</PreviewLine>
      <PreviewLine muted>{"}"}</PreviewLine>
    </div>
  );
}

function AcpProcessPreview({
  commandWords,
  clientOs,
  workspacePath,
  customArgs,
  busy,
  insertionLabel,
  removeLabel,
  onRemoveArg,
}: {
  commandWords: string[];
  clientOs: ClientOs;
  workspacePath: string;
  customArgs: string[];
  busy: boolean;
  insertionLabel: string;
  removeLabel: string;
  onRemoveArg: (index: number) => void;
}) {
  const quote = clientOs === "windows" ? powershellSingleQuoted : quoteCommandWord;
  return (
    <div className="rounded-md border border-border bg-muted/20 px-3 py-2 font-mono text-[11px] leading-5 [font-variant-ligatures:none]">
      <PreviewLine muted>cwd {quote(workspacePath)}</PreviewLine>
      <div className="flex min-h-6 flex-wrap items-center gap-x-1.5 gap-y-1">
        <span className="text-muted-foreground">spawn</span>
        <CommandTokens words={commandWords} quote={quote} />
        <CustomArgTokens
          customArgs={customArgs}
          quote={quote}
          busy={busy}
          insertionLabel={insertionLabel}
          removeLabel={removeLabel}
          onRemoveArg={onRemoveArg}
        />
      </div>
    </div>
  );
}

function CommandTokens({
  words,
  quote,
}: {
  words: string[];
  quote: (word: string) => string;
}) {
  return (
    <>
      {words.map((word, index) => (
        <span
          key={`base-${word}-${index}`}
          className="max-w-full truncate text-foreground"
        >
          {quote(word)}
        </span>
      ))}
    </>
  );
}

function CustomArgTokens({
  customArgs,
  quote,
  busy,
  insertionLabel,
  removeLabel,
  onRemoveArg,
}: {
  customArgs: string[];
  quote: (word: string) => string;
  busy: boolean;
  insertionLabel: string;
  removeLabel: string;
  onRemoveArg: (index: number) => void;
}) {
  if (customArgs.length === 0) return <InsertionToken label={insertionLabel} />;
  return (
    <>
      {customArgs.map((arg, index) => (
        <CustomArgToken
          key={`custom-${arg}-${index}`}
          arg={arg}
          index={index}
          quote={quote}
          busy={busy}
          removeLabel={removeLabel}
          onRemoveArg={onRemoveArg}
        />
      ))}
    </>
  );
}

function CustomArgToken({
  arg,
  index,
  quote,
  busy,
  removeLabel,
  onRemoveArg,
}: {
  arg: string;
  index: number;
  quote: (word: string) => string;
  busy: boolean;
  removeLabel: string;
  onRemoveArg: (index: number) => void;
}) {
  const escapedArg = quote(arg);
  return (
    <span
      className="inline-flex max-w-full min-w-0 items-center gap-1 rounded border border-primary/30 bg-primary/10 px-1.5 text-primary"
      title={escapedArg}
    >
      <span className="truncate">{arg}</span>
      <button
        type="button"
        disabled={busy}
        aria-label={removeLabel}
        title={removeLabel}
        className="rounded-sm text-primary/70 hover:text-primary disabled:pointer-events-none disabled:opacity-50"
        onClick={() => onRemoveArg(index)}
      >
        <X className="h-3 w-3" />
      </button>
    </span>
  );
}

function InsertionToken({ label }: { label: string }) {
  return (
    <span className="inline-flex items-center rounded border border-dashed border-primary/35 bg-primary/5 px-1.5 text-primary/80">
      {label}
    </span>
  );
}

function PreviewLine({
  children,
  muted = false,
}: {
  children: ReactNode;
  muted?: boolean;
}) {
  return (
    <div className={muted ? "text-muted-foreground" : "text-foreground"}>
      {children}
    </div>
  );
}
