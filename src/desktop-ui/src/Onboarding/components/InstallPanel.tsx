import {
  Bot,
  CheckCircle2,
  ChevronDown,
  Circle,
  Globe,
  Loader2,
  MessageSquare,
  TerminalSquare,
} from "lucide-react";
import { useMemo, useState } from "react";

import { cn } from "@/lib/utils";

import {
  groupSummary,
  groupTitle,
  installHeadline,
  StartkitReportRow,
} from "./startkitPresentation";
import type { StartkitChoices, StartkitItemReport } from "../types";

const GROUP_ORDER = ["computer", "agents", "messaging", "remote"];

export function InstallPanel({
  groupedReports,
  reports,
  scanning,
  running,
  complete,
  finalStatus,
  error,
  choices,
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
  tunnelProvider: string;
}) {
  const [showDetails, setShowDetails] = useState(false);
  const groups = useMemo(
    () =>
      GROUP_ORDER.map((id) => ({
        id,
        reports: groupedReports.find((group) => group.id === id)?.reports ?? [],
      })).filter((group) => group.reports.length > 0),
    [groupedReports],
  );
  const needsInput = reports.some((report) => report.status === "needs_config");

  return (
    <div className="mx-auto flex min-h-full w-full max-w-4xl items-center py-8">
      <div className="w-full space-y-4">
        <section
          className={cn(
            "rounded-md border px-5 py-4",
            complete
              ? "border-emerald-500/30 bg-emerald-500/10"
              : running || scanning
                ? "border-primary/30 bg-primary/10"
                : "border-border bg-card",
          )}
        >
          <div className="flex items-start gap-3">
            {running || scanning ? (
              <Loader2 className="mt-0.5 h-5 w-5 animate-spin text-primary" />
            ) : complete ? (
              <CheckCircle2 className="mt-0.5 h-5 w-5 text-emerald-600" />
            ) : (
              <Circle className="mt-0.5 h-5 w-5 text-muted-foreground" />
            )}
            <div>
              <div className="text-base font-semibold">
                {installHeadline({ scanning, running, complete, finalStatus })}
              </div>
              <p className="mt-1 text-sm text-muted-foreground">
                {needsInput
                  ? "Some items only need configuration in the next step."
                  : "Ready items are skipped automatically."}
              </p>
            </div>
          </div>
        </section>

        {error && (
          <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive">
            {error}
          </div>
        )}

        {groups.length === 0 ? (
          <div className="flex min-h-[220px] items-center justify-center rounded-md border border-dashed border-border bg-card">
            <div className="max-w-sm text-center">
              <Loader2 className="mx-auto mb-3 h-7 w-7 animate-spin text-primary" />
              <div className="text-sm font-medium">Preparing setup plan</div>
              <p className="mt-1 text-xs text-muted-foreground">
                The environment check starts automatically.
              </p>
            </div>
          </div>
        ) : (
          <div className="grid gap-2 sm:grid-cols-2">
            {groups.map((group) => (
              <SetupGroupCard
                key={group.id}
                id={group.id}
                reports={group.reports}
                choices={choices}
                tunnelProvider={tunnelProvider}
              />
            ))}
          </div>
        )}

        {groups.length > 0 && (
          <section className="rounded-md border border-border bg-card">
            <button
              type="button"
              className="flex w-full items-center justify-between gap-3 px-4 py-3 text-left"
              onClick={() => setShowDetails((value) => !value)}
            >
              <span className="text-sm font-medium">Details</span>
              <ChevronDown
                className={cn(
                  "h-4 w-4 text-muted-foreground transition-transform",
                  showDetails && "rotate-180",
                )}
              />
            </button>
            {showDetails && (
              <div className="space-y-3 border-t border-border p-3 animate-in fade-in slide-in-from-top-1 duration-200">
                {groups.map((group) => (
                  <div
                    key={group.id}
                    className="overflow-hidden rounded-md border border-border bg-background"
                  >
                    <div className="border-b border-border bg-muted/30 px-3 py-2 text-xs font-medium">
                      {groupTitle(group.id)}
                    </div>
                    <div className="divide-y divide-border">
                      {group.reports.map((report) => (
                        <StartkitReportRow key={report.id} report={report} />
                      ))}
                    </div>
                  </div>
                ))}
              </div>
            )}
          </section>
        )}
      </div>
    </div>
  );
}

function SetupGroupCard({
  id,
  reports,
  choices,
  tunnelProvider,
}: {
  id: string;
  reports: StartkitItemReport[];
  choices: StartkitChoices;
  tunnelProvider: string;
}) {
  const status = groupStatus(reports);
  return (
    <div className="rounded-md border border-border bg-card p-4">
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 items-start gap-3">
          <div className="mt-0.5 text-primary">{groupIcon(id)}</div>
          <div className="min-w-0">
            <div className="text-sm font-medium">{groupTitle(id)}</div>
            <div className="mt-1 truncate text-xs text-muted-foreground">
              {groupDetail(id, choices, tunnelProvider)}
            </div>
          </div>
        </div>
        <span className={cn("shrink-0 rounded-full border px-2 py-1 text-[11px]", status.className)}>
          {status.label}
        </span>
      </div>
      <div className="mt-3 text-xs text-muted-foreground">
        {groupSummary(reports)}
      </div>
    </div>
  );
}

function groupStatus(reports: StartkitItemReport[]) {
  if (reports.some((report) => report.status === "running")) {
    return {
      label: "Working",
      className: "border-primary/30 bg-primary/10 text-primary",
    };
  }
  if (reports.some((report) => report.status === "error" || report.status === "blocked")) {
    return {
      label: "Needs attention",
      className: "border-destructive/30 bg-destructive/10 text-destructive",
    };
  }
  if (reports.some((report) => report.status === "needs_config")) {
    return {
      label: "Configure",
      className: "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300",
    };
  }
  if (
    reports.some((report) =>
      report.status === "missing" ||
      report.status === "outdated" ||
      report.status === "broken" ||
      report.actions.includes("install"),
    )
  ) {
    return {
      label: "Will install",
      className: "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300",
    };
  }
  if (reports.every((report) => report.status === "ok" || report.status === "skipped")) {
    return {
      label: "Ready",
      className: "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
    };
  }
  return {
    label: "Checking",
    className: "border-border bg-muted text-muted-foreground",
  };
}

function groupIcon(id: string) {
  const className = "h-4 w-4";
  switch (id) {
    case "agents":
      return <Bot className={className} />;
    case "messaging":
      return <MessageSquare className={className} />;
    case "remote":
      return <Globe className={className} />;
    default:
      return <TerminalSquare className={className} />;
  }
}

function groupDetail(
  id: string,
  choices: StartkitChoices,
  tunnelProvider: string,
): string {
  switch (id) {
    case "agents":
      return choices.agents.length > 0
        ? `${choices.agents.length} selected`
        : "Skipped";
    case "messaging":
      return choices.channels.length > 0
        ? `${choices.channels.length} selected`
        : "Skipped";
    case "remote":
      return tunnelProvider === "none" ? "Skipped" : tunnelProvider;
    default:
      return choices.source === "cn" ? "China mirror" : "Global source";
  }
}
