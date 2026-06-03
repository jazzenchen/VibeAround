import {
  Bot,
  CheckCircle2,
  ChevronDown,
  Globe,
  Loader2,
  SlidersHorizontal,
  TerminalSquare,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { BrandIcon } from "@/components/brand-icon";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";

import { PanelSection } from "./PanelSection";
import { compactReportLabel } from "./startkitPresentation";
import type {
  AgentSummary,
  StartkitItemReport,
  StartkitManifestSummary,
} from "../types";
import type { AgentId } from "../constants";

export function AgentDecisionPanel({
  agents,
  enabledAgents,
  reports,
  scanning,
  toolchainMode,
  onToolchainMode,
  sources,
  downloadSource,
  onDownloadSource,
  shellPath,
  shellPathDisabled,
  onShellPath,
  onToggleAgent,
}: {
  agents: AgentSummary[];
  enabledAgents: Set<AgentId>;
  reports: Map<string, StartkitItemReport>;
  scanning: boolean;
  toolchainMode: "auto" | "managed" | "system";
  onToolchainMode: (value: "auto" | "managed" | "system") => void;
  sources: StartkitManifestSummary["sources"];
  downloadSource: string;
  onDownloadSource: (value: string) => void;
  shellPath: boolean;
  shellPathDisabled: boolean;
  onShellPath: (checked: boolean) => void;
  onToggleAgent: (id: AgentId) => void;
}) {
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [showMoreAgents, setShowMoreAgents] = useState(false);

  useEffect(() => {
    if (toolchainMode !== "auto") onToolchainMode("auto");
  }, [toolchainMode, onToolchainMode]);

  const recommendedAgents = useMemo(
    () => agents.filter((agent) => agent.id === "claude" || agent.id === "codex"),
    [agents],
  );
  const otherAgents = useMemo(
    () => agents.filter((agent) => agent.id !== "claude" && agent.id !== "codex"),
    [agents],
  );

  return (
    <div className="mx-auto flex min-h-full w-full max-w-4xl items-center py-8">
      <div className="w-full space-y-4">
        <section className="rounded-md border border-border bg-card p-5">
          <div className="flex items-start gap-4">
            <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-md bg-primary/10 text-primary">
              <CheckCircle2 className="h-5 w-5" />
            </div>
            <div className="min-w-0 flex-1">
              <div className="text-base font-semibold">Auto setup</div>
              <p className="mt-1 max-w-2xl text-sm leading-6 text-muted-foreground">
                VibeAround checks Node, Git, Claude, and Codex. Existing system
                tools are reused; missing tools are prepared in the managed
                directory.
              </p>
            </div>
          </div>
        </section>

        <PanelSection
          icon={<Bot className="h-4 w-4" />}
          title="Agents to enable"
          description="Claude and Codex are selected by default."
        >
          <AgentGrid
            agents={recommendedAgents}
            enabled={enabledAgents}
            reports={reports}
            onToggle={onToggleAgent}
          />

          {otherAgents.length > 0 && (
            <div className="mt-3">
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="px-0 text-xs text-muted-foreground hover:bg-transparent"
                onClick={() => setShowMoreAgents((value) => !value)}
              >
                <ChevronDown
                  className={cn(
                    "h-3.5 w-3.5 transition-transform",
                    showMoreAgents && "rotate-180",
                  )}
                />
                {showMoreAgents ? "Hide more agents" : "More agents"}
              </Button>
              {showMoreAgents && (
                <div className="mt-2 animate-in fade-in slide-in-from-top-1 duration-200">
                  <AgentGrid
                    agents={otherAgents}
                    enabled={enabledAgents}
                    reports={reports}
                    onToggle={onToggleAgent}
                  />
                </div>
              )}
            </div>
          )}
        </PanelSection>

        <DetectionSummary reports={reports} scanning={scanning} />

        <section className="rounded-md border border-border bg-card">
          <button
            type="button"
            className="flex w-full items-center justify-between gap-3 px-4 py-3 text-left"
            onClick={() => setShowAdvanced((value) => !value)}
          >
            <span className="flex items-center gap-2 text-sm font-medium">
              <SlidersHorizontal className="h-4 w-4 text-primary" />
              Advanced
            </span>
            <ChevronDown
              className={cn(
                "h-4 w-4 text-muted-foreground transition-transform",
                showAdvanced && "rotate-180",
              )}
            />
          </button>
          {showAdvanced && (
            <div className="grid gap-3 border-t border-border p-4 lg:grid-cols-2 animate-in fade-in slide-in-from-top-1 duration-200">
              <SourceChooser
                sources={sources}
                value={downloadSource}
                onChange={onDownloadSource}
              />
              <ShellPathChooser
                checked={shellPath}
                disabled={shellPathDisabled}
                onChange={onShellPath}
              />
            </div>
          )}
        </section>
      </div>
    </div>
  );
}

function DetectionSummary({
  reports,
  scanning,
}: {
  reports: Map<string, StartkitItemReport>;
  scanning: boolean;
}) {
  const visibleIds = [
    "essentials.node",
    "essentials.git",
    "agents.claude.cli",
    "agents.codex.cli",
  ];
  const visibleReports = visibleIds
    .map((id) => reports.get(id))
    .filter((report): report is StartkitItemReport => Boolean(report));
  const ready = visibleReports.filter((report) => report.status === "ok").length;
  const needsSetup = visibleReports.filter((report) =>
    ["missing", "outdated", "broken"].includes(report.status),
  ).length;

  return (
    <section className="rounded-md border border-border bg-card p-4">
      <div className="flex items-center justify-between gap-4">
        <div className="min-w-0">
          <div className="flex items-center gap-2 text-sm font-medium">
            {scanning ? (
              <Loader2 className="h-4 w-4 animate-spin text-primary" />
            ) : (
              <TerminalSquare className="h-4 w-4 text-primary" />
            )}
            Environment check
          </div>
          <p className="mt-1 text-xs text-muted-foreground">
            {scanning
              ? "Checking in the background."
              : visibleReports.length === 0
                ? "Detection starts automatically."
                : `${ready} ready, ${needsSetup} to prepare.`}
          </p>
        </div>
        <div className="hidden items-center gap-2 text-xs text-muted-foreground sm:flex">
          {visibleReports.slice(0, 4).map((report) => (
            <span
              key={report.id}
              className={cn(
                "rounded-full border px-2 py-1",
                report.status === "ok"
                  ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300"
                  : "border-border bg-muted",
              )}
            >
              {report.label}
            </span>
          ))}
        </div>
      </div>
    </section>
  );
}

function SourceChooser({
  sources,
  value,
  onChange,
}: {
  sources: StartkitManifestSummary["sources"];
  value: string;
  onChange: (value: string) => void;
}) {
  const entries: Array<[string, { label: string }]> =
    Object.keys(sources).length > 0
      ? Object.entries(sources)
      : [
          ["global", { label: "Global" }],
          ["cn", { label: "China mirror" }],
        ];
  return (
    <div className="rounded-md border border-border bg-background p-3">
      <div className="mb-2 flex items-center gap-2 text-xs font-medium">
        <Globe className="h-3.5 w-3.5 text-primary" />
        Download source
      </div>
      <div className="grid grid-cols-2 gap-2">
        {entries.map(([id, source]) => (
          <Button
            key={id}
            type="button"
            size="sm"
            variant="outline"
            className={cn(
              "justify-center text-xs",
              value === id && "border-primary bg-primary/10 text-primary",
            )}
            onClick={() => onChange(id)}
          >
            {source.label}
          </Button>
        ))}
      </div>
    </div>
  );
}

function ShellPathChooser({
  checked,
  disabled,
  onChange,
}: {
  checked: boolean;
  disabled: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <div
      className={cn(
        "rounded-md border border-border bg-background p-3",
        disabled && "opacity-60",
      )}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2 text-xs font-medium">
            <TerminalSquare className="h-3.5 w-3.5 text-primary" />
            Write shell PATH
          </div>
          <p className="mt-1 text-[11px] leading-snug text-muted-foreground">
            Terminal sessions can find managed Node, Codex, Claude, and helper tools.
          </p>
        </div>
        <Switch
          checked={checked}
          disabled={disabled}
          onCheckedChange={onChange}
          aria-label="Write shell PATH"
        />
      </div>
    </div>
  );
}

function AgentGrid({
  agents,
  enabled,
  reports,
  onToggle,
}: {
  agents: AgentSummary[];
  enabled: Set<string>;
  reports: Map<string, StartkitItemReport>;
  onToggle: (id: string) => void;
}) {
  return (
    <div className="grid gap-2 sm:grid-cols-2">
      {agents.map((agent) => {
        const selected = enabled.has(agent.id);
        const report = reports.get(`agents.${agent.id}.cli`);
        return (
          <button
            key={agent.id}
            type="button"
            className={cn(
              "relative flex min-h-[68px] items-center gap-3 rounded-md border p-3 pr-9 text-left transition-colors",
              selected
                ? "border-primary/50 bg-primary/10"
                : "border-border bg-background hover:border-primary/30",
            )}
            onClick={() => onToggle(agent.id)}
          >
            <BrandIcon
              kind="cli"
              id={agent.id}
              label={agent.display_name}
              className="h-8 w-8"
            />
            <span className="min-w-0 flex-1">
              <span className="block truncate text-sm font-medium">
                {agent.display_name}
              </span>
              <span className="mt-0.5 block truncate text-[11px] text-muted-foreground">
                {report ? compactReportLabel(report) : agent.install_type ?? "CLI"}
              </span>
            </span>
            <Checkbox
              checked={selected}
              aria-hidden="true"
              tabIndex={-1}
              className="pointer-events-none absolute right-3 top-1/2 -translate-y-1/2"
            />
          </button>
        );
      })}
    </div>
  );
}
