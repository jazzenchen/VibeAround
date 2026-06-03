import {
  Bot,
  CheckCircle2,
  Circle,
  Globe,
  Loader2,
  MessageSquare,
  TerminalSquare,
} from "lucide-react";
import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

import {
  groupIcon,
  groupSummary,
  groupTitle,
  installHeadline,
  StartkitReportRow,
} from "./startkitPresentation";
import type {
  AgentSummary,
  PluginRegistryEntry,
  StartkitChoices,
  StartkitItemReport,
} from "../types";

export function InstallPanel({
  groupedReports,
  reports,
  scanning,
  running,
  complete,
  finalStatus,
  error,
  choices,
  agents,
  pluginRegistry,
  tunnelProvider,
}: {
  groupedReports: Array<{ id: string; reports: StartkitItemReport[] }>;
  reports: StartkitItemReport[];
  scanning: boolean;
  running: boolean;
  complete: boolean;
  finalStatus: string | null;
  error: string | null;
  choices: StartkitChoices;
  agents: AgentSummary[];
  pluginRegistry: PluginRegistryEntry[];
  tunnelProvider: string;
}) {
  const selectedAgents = agents.filter((agent) => choices.agents.includes(agent.id));
  const selectedPlugins = pluginRegistry.filter((plugin) =>
    choices.channels.includes(plugin.id),
  );
  const needsInput = reports.some((report) => report.status === "needs_config");

  return (
    <div className="mx-auto w-full max-w-6xl space-y-5">
      <div className="grid gap-3 lg:grid-cols-4">
        <SetupSummaryCard
          icon={<TerminalSquare className="h-4 w-4" />}
          label="Basics"
          value={choices.toolchainMode}
          detail={choices.source === "cn" ? "China mirror" : "Global source"}
        />
        <SetupSummaryCard
          icon={<Bot className="h-4 w-4" />}
          label="Agents"
          value={
            selectedAgents.length > 0
              ? `${selectedAgents.length} selected`
              : "Skipped"
          }
          detail={selectedAgents.map((agent) => agent.display_name).join(", ")}
        />
        <SetupSummaryCard
          icon={<MessageSquare className="h-4 w-4" />}
          label="IM"
          value={
            selectedPlugins.length > 0
              ? `${selectedPlugins.length} selected`
              : "Skipped"
          }
          detail={selectedPlugins.map((plugin) => plugin.name).join(", ")}
        />
        <SetupSummaryCard
          icon={<Globe className="h-4 w-4" />}
          label="Remote"
          value={tunnelProvider === "none" ? "Skipped" : tunnelProvider}
          detail={tunnelProvider === "cloudflare" ? "Recommended" : ""}
        />
      </div>

      <div
        className={cn(
          "rounded-md border px-4 py-3",
          complete
            ? "border-emerald-500/30 bg-emerald-500/10"
            : running || scanning
              ? "border-primary/30 bg-primary/10"
              : needsInput
                ? "border-amber-500/30 bg-amber-500/10"
                : "border-border bg-muted/30",
        )}
      >
        <div className="flex items-start gap-2">
          {running || scanning ? (
            <Loader2 className="mt-0.5 h-4 w-4 animate-spin text-primary" />
          ) : complete ? (
            <CheckCircle2 className="mt-0.5 h-4 w-4 text-emerald-600" />
          ) : (
            <Circle className="mt-0.5 h-4 w-4 text-muted-foreground" />
          )}
          <div>
            <div className="text-sm font-medium">
              {installHeadline({ scanning, running, complete, finalStatus })}
            </div>
            <p className="mt-1 text-xs text-muted-foreground">
              {needsInput
                ? "Configuration-only items are handled in the next step."
                : "Startkit follows the plan below and skips items already ready."}
            </p>
          </div>
        </div>
      </div>

      {error && (
        <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive">
          {error}
        </div>
      )}

      {groupedReports.length === 0 ? (
        <div className="flex min-h-[320px] items-center justify-center rounded-md border border-dashed border-border">
          <div className="max-w-sm text-center">
            <Loader2 className="mx-auto mb-3 h-8 w-8 animate-spin text-primary" />
            <div className="text-sm font-medium">Preparing setup plan</div>
            <p className="mt-1 text-xs text-muted-foreground">
              The environment check starts automatically from your selections.
            </p>
          </div>
        </div>
      ) : (
        <div className="space-y-4">
          {groupedReports.map((group) => (
            <section
              key={group.id}
              className="overflow-hidden rounded-md border border-border bg-background"
            >
              <div className="flex items-center justify-between gap-3 border-b border-border bg-muted/30 px-4 py-3">
                <div className="flex items-center gap-2">
                  {groupIcon(group.id)}
                  <div>
                    <div className="text-sm font-medium">
                      {groupTitle(group.id)}
                    </div>
                    <div className="text-[11px] text-muted-foreground">
                      {groupSummary(group.reports)}
                    </div>
                  </div>
                </div>
              </div>
              <div className="divide-y divide-border">
                {group.reports.map((report) => (
                  <StartkitReportRow key={report.id} report={report} />
                ))}
              </div>
            </section>
          ))}
        </div>
      )}
    </div>
  );
}

function SetupSummaryCard({
  icon,
  label,
  value,
  detail,
}: {
  icon: ReactNode;
  label: string;
  value: string;
  detail?: string;
}) {
  return (
    <div className="rounded-md border border-border bg-background p-3">
      <div className="flex items-center gap-2 text-xs font-medium text-muted-foreground">
        <span className="text-primary">{icon}</span>
        {label}
      </div>
      <div className="mt-2 truncate text-sm font-semibold">{value}</div>
      {detail && (
        <div className="mt-0.5 truncate text-[11px] text-muted-foreground">
          {detail}
        </div>
      )}
    </div>
  );
}
