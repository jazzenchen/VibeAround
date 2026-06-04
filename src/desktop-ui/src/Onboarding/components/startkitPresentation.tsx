import {
  AlertCircle,
  CheckCircle2,
  Circle,
  Globe,
  Loader2,
  MessageSquare,
  Settings2,
  TerminalSquare,
} from "lucide-react";

import { cn } from "@/lib/utils";

import type { StartkitItemReport, StartkitStatus } from "../types";

const GROUP_ORDER = ["computer", "agents", "messaging", "remote"];

export function StartkitReportRow({
  report,
  compact = false,
}: {
  report: StartkitItemReport;
  compact?: boolean;
}) {
  return (
    <div
      className={cn(
        "grid items-center gap-3 px-4 py-3",
        compact
          ? "grid-cols-[minmax(140px,1fr)_112px]"
          : "grid-cols-[minmax(180px,1fr)_120px_minmax(180px,1.3fr)]",
      )}
    >
      <div className="min-w-0">
        <div className="truncate text-sm font-medium">{report.label}</div>
        {report.path && (
          <div className="mt-0.5 truncate font-mono text-[10px] text-muted-foreground">
            {shortenHome(report.path)}
          </div>
        )}
      </div>
      <StatusPill report={report} />
      {!compact && (
        <div className="min-w-0 text-xs text-muted-foreground">
          <div className="truncate">
            {report.message ?? report.version ?? "Waiting for check"}
          </div>
          {report.version && report.message && (
            <div className="mt-0.5 truncate font-mono text-[10px] opacity-80">
              {report.version}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export function groupReports(
  planItems: Array<{
    id: string;
    group: string;
    label: string;
    category: string;
    severity?: string;
    secret: boolean;
    settingsKey?: string;
  }>,
  reportById: Map<string, StartkitItemReport>,
) {
  const groups = new Map<string, StartkitItemReport[]>();
  for (const item of planItems) {
    const report =
      reportById.get(item.id) ??
      ({
        id: item.id,
        label: item.label,
        group: item.group,
        category: item.category,
        status: "pending",
        severity: item.severity,
        actions: [],
        secret: item.secret,
        settingsKey: item.settingsKey,
      } satisfies StartkitItemReport);
    if (!groups.has(report.group)) groups.set(report.group, []);
    groups.get(report.group)!.push(report);
  }
  return Array.from(groups.entries())
    .sort(
      ([a], [b]) =>
        (GROUP_ORDER.indexOf(a) < 0 ? 99 : GROUP_ORDER.indexOf(a)) -
        (GROUP_ORDER.indexOf(b) < 0 ? 99 : GROUP_ORDER.indexOf(b)),
    )
    .map(([id, groupReports]) => ({ id, reports: groupReports }));
}

export function groupTitle(id: string): string {
  switch (id) {
    case "computer":
      return "Computer basics";
    case "agents":
      return "Coding agents";
    case "remote":
      return "Remote access";
    case "messaging":
      return "Messaging";
    default:
      return id;
  }
}

export function groupIcon(id: string) {
  const className = "h-4 w-4 text-primary";
  switch (id) {
    case "agents":
      return <TerminalSquare className={className} />;
    case "remote":
      return <Globe className={className} />;
    case "messaging":
      return <MessageSquare className={className} />;
    default:
      return <Settings2 className={className} />;
  }
}

export function groupSummary(reports: StartkitItemReport[]): string {
  const counts = reports.reduce<Record<string, number>>((acc, report) => {
    acc[report.status] = (acc[report.status] ?? 0) + 1;
    return acc;
  }, {});
  if (counts.error || counts.blocked) return "Needs attention";
  if (counts.running) return groupActivityLabel(reports);
  if (counts.needs_config) return "Configure later";
  if (counts.missing || counts.outdated || counts.broken) return "Setup available";
  if (counts.ok && counts.ok === reports.length) return "Ready";
  return `${reports.length} item${reports.length === 1 ? "" : "s"}`;
}

export function reportNeedsInstall(report: StartkitItemReport): boolean {
  return (
    report.status === "missing" ||
    report.status === "outdated" ||
    report.status === "broken" ||
    report.actions.includes("install")
  );
}

export function compactReportLabel(report: StartkitItemReport): string {
  if (report.status === "ok") {
    return report.version ?? report.path ?? "Installed";
  }
  if (report.status === "pending") return "Checking";
  if (report.status === "missing") return "Not installed";
  if (report.status === "outdated") return "Outdated";
  if (report.status === "broken") return "Needs repair";
  if (report.status === "needs_config") return "Needs config";
  return statusLabel(report.status);
}

export function installHeadline({
  scanning,
  running,
  complete,
  finalStatus,
}: {
  scanning: boolean;
  running: boolean;
  complete: boolean;
  finalStatus: string | null;
}) {
  if (running) return "Installing selected setup";
  if (scanning) return "Checking this computer";
  if (complete && finalStatus === "error") return "Setup finished with issues";
  if (complete) return "Setup run finished";
  return "Ready to prepare selected items";
}

export function tunnelRank(id: string): number {
  switch (id) {
    case "cloudflare":
      return 0;
    case "none":
      return 1;
    case "ngrok":
      return 2;
    case "localtunnel":
      return 3;
    default:
      return 10;
  }
}

export function tunnelDescription(id: string): string {
  switch (id) {
    case "cloudflare":
      return "Stable named tunnel with a public hostname.";
    case "ngrok":
      return "Useful when you already have an ngrok account and domain.";
    case "localtunnel":
      return "Quick temporary public URL for lightweight testing.";
    case "none":
      return "Keep everything local on this computer.";
    default:
      return "Remote access provider.";
  }
}

function StatusPill({ report }: { report: StartkitItemReport }) {
  return (
    <div
      className={cn(
        "inline-flex w-fit items-center gap-1.5 rounded border px-2 py-1 text-[11px]",
        statusClass(report.status),
      )}
    >
      {statusIcon(report.status)}
      {reportStatusLabel(report)}
    </div>
  );
}

function statusClass(status: StartkitStatus): string {
  switch (status) {
    case "ok":
      return "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
    case "running":
      return "border-primary/30 bg-primary/10 text-primary";
    case "missing":
    case "outdated":
    case "broken":
    case "needs_config":
      return "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300";
    case "blocked":
    case "error":
      return "border-destructive/30 bg-destructive/10 text-destructive";
    case "skipped":
      return "border-border bg-muted text-muted-foreground";
    default:
      return "border-border bg-background text-muted-foreground";
  }
}

function statusIcon(status: StartkitStatus) {
  const className = "h-3.5 w-3.5";
  switch (status) {
    case "ok":
      return <CheckCircle2 className={className} />;
    case "running":
      return <Loader2 className={`${className} animate-spin`} />;
    case "blocked":
    case "error":
      return <AlertCircle className={className} />;
    default:
      return <Circle className={className} />;
  }
}

function statusLabel(status: StartkitStatus): string {
  switch (status) {
    case "needs_config":
      return "needs config";
    default:
      return status.replace("_", " ");
  }
}

function reportStatusLabel(report: StartkitItemReport): string {
  return report.status === "running"
    ? reportActivityLabel(report)
    : statusLabel(report.status);
}

function groupActivityLabel(reports: StartkitItemReport[]): string {
  const activeLabels = reports
    .map((report) =>
      report.status === "running" ? reportActivityLabel(report) : null,
    )
    .filter((label): label is string => Boolean(label));

  if (activeLabels.includes("downloading")) return "Downloading";
  if (activeLabels.includes("installing")) return "Installing";
  if (activeLabels.includes("updating")) return "Updating";
  if (activeLabels.includes("checking")) return "Checking";
  return "Working";
}

function reportActivityLabel(report: StartkitItemReport): string {
  const message = (report.message ?? "").toLowerCase();
  if (message.includes("download")) return "downloading";
  if (
    message.includes("install") ||
    message.includes("npm") ||
    message.includes("clone") ||
    message.includes("build")
  ) {
    return "installing";
  }
  if (message.includes("path") || message.includes("updat")) return "updating";
  if (message.includes("check") || message.includes("scan")) return "checking";
  return "working";
}

function shortenHome(path: string): string {
  return path.replace(/^\/Users\/[^/]+/, "~").replace(/^C:\\Users\\[^\\]+/i, "~");
}
