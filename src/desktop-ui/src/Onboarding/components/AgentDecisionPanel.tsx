import {
  Bot,
  Globe,
  Loader2,
  ShieldCheck,
  TerminalSquare,
} from "lucide-react";

import { BrandIcon } from "@/components/brand-icon";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";

import { PanelSection } from "./PanelSection";
import {
  compactReportLabel,
  StartkitReportRow,
} from "./startkitPresentation";
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
  return (
    <div className="mx-auto w-full max-w-5xl space-y-5">
      <PanelSection
        icon={<TerminalSquare className="h-4 w-4" />}
        title="Install preference"
        description="Auto is the safest default: system tools win when valid, managed tools fill the gaps."
      >
        <ToolchainChooser value={toolchainMode} onChange={onToolchainMode} />
        <div className="mt-4 grid gap-3 lg:grid-cols-2">
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
      </PanelSection>

      <PanelSection
        icon={<Bot className="h-4 w-4" />}
        title="Coding agents"
        description="Select the CLIs VibeAround should launch from the app and IM messages."
      >
        <AgentGrid
          agents={agents}
          enabled={enabledAgents}
          reports={reports}
          onToggle={onToggleAgent}
        />
      </PanelSection>

      <PanelSection
        icon={<ShieldCheck className="h-4 w-4" />}
        title="Detected tools"
        description="This refreshes automatically as your choices change."
      >
        <DetectedToolList reports={reports} scanning={scanning} />
      </PanelSection>
    </div>
  );
}

function ToolchainChooser({
  value,
  onChange,
}: {
  value: "auto" | "managed" | "system";
  onChange: (value: "auto" | "managed" | "system") => void;
}) {
  const options: Array<{
    id: "auto" | "managed" | "system";
    label: string;
    description: string;
  }> = [
    {
      id: "auto",
      label: "Auto prepare",
      description: "Reuse valid system tools; install managed copies only when needed.",
    },
    {
      id: "system",
      label: "Already installed",
      description: "Use PATH tools only; Startkit will report anything missing.",
    },
    {
      id: "managed",
      label: "Managed install",
      description: "Install and prefer VibeAround-managed Node and agent CLIs.",
    },
  ];

  return (
    <div className="grid gap-2 md:grid-cols-3">
      {options.map((option) => (
        <button
          key={option.id}
          type="button"
          className={cn(
            "min-h-[92px] rounded-md border p-3 text-left transition-colors",
            value === option.id
              ? "border-primary bg-primary/10 text-foreground"
              : "border-border bg-background hover:border-primary/30",
          )}
          onClick={() => onChange(option.id)}
        >
          <span className="block text-sm font-medium">{option.label}</span>
          <span className="mt-1 block text-xs leading-5 text-muted-foreground">
            {option.description}
          </span>
        </button>
      ))}
    </div>
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
    <div className="grid grid-cols-[repeat(auto-fill,minmax(190px,1fr))] gap-2">
      {agents.map((agent) => {
        const selected = enabled.has(agent.id);
        const report = reports.get(`agents.${agent.id}.cli`);
        return (
          <button
            key={agent.id}
            type="button"
            className={cn(
              "relative flex min-h-[74px] items-center gap-3 rounded-md border p-3 pr-9 text-left transition-colors",
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

function DetectedToolList({
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
    "environment.shell_path",
  ];
  const visibleReports = visibleIds
    .map((id) => reports.get(id))
    .filter((report): report is StartkitItemReport => Boolean(report));

  if (visibleReports.length === 0) {
    return (
      <div className="flex min-h-[96px] items-center justify-center rounded-md border border-dashed border-border text-xs text-muted-foreground">
        {scanning ? (
          <>
            <Loader2 className="mr-2 h-3.5 w-3.5 animate-spin" />
            Checking selected tools...
          </>
        ) : (
          "Detection starts automatically."
        )}
      </div>
    );
  }

  return (
    <div className="divide-y divide-border rounded-md border border-border bg-background">
      {visibleReports.map((report) => (
        <StartkitReportRow key={report.id} report={report} compact />
      ))}
    </div>
  );
}
