import {
  Bot,
  CheckCircle2,
  ChevronDown,
  Globe,
  Loader2,
  MessageSquare,
  TerminalSquare,
} from "lucide-react";
import { useI18n } from "@va/i18n";
import { useMemo, useState } from "react";

import { cn } from "@/lib/utils";

import {
  groupSummary,
  installHeadline,
  installProgressLabel,
  StartkitReportRow,
  translatedGroupTitle,
} from "./startkitPresentation";
import type {
  DiscoveredChannelPlugin,
  PluginRegistryEntry,
  StartkitChoices,
  StartkitItemReport,
  StartkitStatus,
} from "../types";

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
  pluginRegistry,
  discoveredPlugins,
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
  pluginRegistry: PluginRegistryEntry[];
  discoveredPlugins: DiscoveredChannelPlugin[];
}) {
  const { t } = useI18n();
  const [showDetails, setShowDetails] = useState(false);
  const installReports = useMemo(
    () => reports.filter(isInstallStepReport),
    [reports],
  );
  const groups = useMemo(
    () =>
      GROUP_ORDER.map((id) => ({
        id,
        reports:
          groupedReports
            .find((group) => group.id === id)
            ?.reports.filter(isInstallStepReport) ?? [],
      })).filter((group) => group.reports.length > 0),
    [groupedReports],
  );
  const detailGroups = useMemo(
    () =>
      groups.map((group) => ({
        id: group.id,
        reports:
          group.id === "messaging"
            ? messagingDetailReports(
                group.reports,
                choices,
                pluginRegistry,
                discoveredPlugins,
              )
            : group.reports,
      })),
    [choices, discoveredPlugins, groups, pluginRegistry],
  );
  const headline = running
    ? installProgressLabel(installReports, t)
    : installHeadline({ scanning, running, complete, finalStatus, t });

  return (
    <div className="mx-auto flex min-h-full w-full max-w-4xl items-center py-4">
      <div className="w-full space-y-4">
        <section className="px-1">
          <div className="flex items-start gap-3">
            {running || scanning ? (
              <Loader2 className="mt-0.5 h-5 w-5 animate-spin text-primary" />
            ) : complete ? (
              <CheckCircle2 className="mt-0.5 h-5 w-5 text-emerald-600" />
            ) : (
              <TerminalSquare className="mt-0.5 h-5 w-5 text-primary" />
            )}
            <div>
              <div className="text-base font-semibold">
                {headline}
              </div>
            </div>
          </div>
        </section>

        {error && (
          <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive">
            {error}
          </div>
        )}

        {groups.length === 0 ? (
          <div className="flex min-h-[220px] items-center justify-center">
            <div className="max-w-sm text-center">
              <Loader2 className="mx-auto mb-3 h-7 w-7 animate-spin text-primary" />
              <div className="text-sm font-medium">{t("Preparing setup plan")}</div>
              <p className="mt-1 text-xs text-muted-foreground">
                {t("The environment check starts automatically.")}
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
                t={t}
              />
            ))}
          </div>
        )}

        {groups.length > 0 && (
          <section className="space-y-3">
            <button
              type="button"
              className="flex w-full items-center gap-3 text-left"
              onClick={() => setShowDetails((value) => !value)}
            >
              <span className="h-px flex-1 bg-border" aria-hidden="true" />
              <span className="flex shrink-0 items-center gap-1.5 text-xs font-medium text-muted-foreground">
                {t("Details")}
                <ChevronDown
                  className={cn(
                    "h-3.5 w-3.5 transition-transform",
                    showDetails && "rotate-180",
                  )}
                />
              </span>
              <span className="h-px flex-1 bg-border" aria-hidden="true" />
            </button>
            {showDetails && (
              <div className="space-y-3 animate-in fade-in slide-in-from-top-1 duration-200">
                {detailGroups.map((group) => (
                  <div
                    key={group.id}
                    className="overflow-hidden rounded-md border border-border bg-background"
                  >
                    <div className="border-b border-border bg-muted/30 px-3 py-2 text-xs font-medium">
                      {translatedGroupTitle(group.id, t)}
                    </div>
                    <div className="divide-y divide-border">
                      {group.reports.map((report) => (
                        <StartkitReportRow key={report.id} report={report} t={t} />
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
  t,
}: {
  id: string;
  reports: StartkitItemReport[];
  choices: StartkitChoices;
  tunnelProvider: string;
  t: (key: string, params?: Record<string, string | number>) => string;
}) {
  const status = groupStatus(reports, t);
  return (
    <div className="rounded-md border border-border bg-card p-4">
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 items-start gap-3">
          <div className="mt-0.5 text-primary">{groupIcon(id)}</div>
          <div className="min-w-0">
            <div className="text-sm font-medium">{translatedGroupTitle(id, t)}</div>
            <div className="mt-1 truncate text-xs text-muted-foreground">
              {groupDetail(id, choices, tunnelProvider, t)}
            </div>
          </div>
        </div>
        <span className={cn("shrink-0 rounded-full border px-2 py-1 text-[11px]", status.className)}>
          {status.label}
        </span>
      </div>
      <div className="mt-3 text-xs text-muted-foreground">
        {groupSummary(reports, t)}
      </div>
    </div>
  );
}

function groupStatus(
  reports: StartkitItemReport[],
  t: (key: string, params?: Record<string, string | number>) => string,
) {
  if (reports.some((report) => report.status === "running")) {
    return {
      label: installProgressLabel(reports, t),
      className: "border-primary/30 bg-primary/10 text-primary",
    };
  }
  if (reports.some((report) => report.status === "error" || report.status === "blocked")) {
    return {
      label: t("Needs attention"),
      className: "border-destructive/30 bg-destructive/10 text-destructive",
    };
  }
  if (reports.some((report) => report.status === "needs_config")) {
    return {
      label: t("Configure"),
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
      label: t("Will install"),
      className: "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300",
    };
  }
  if (reports.every((report) => report.status === "ok" || report.status === "skipped")) {
    return {
      label: t("Installed"),
      className: "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
    };
  }
  return {
    label: t("Checking"),
    className: "border-border bg-muted text-muted-foreground",
  };
}

function messagingDetailReports(
  reports: StartkitItemReport[],
  choices: StartkitChoices,
  pluginRegistry: PluginRegistryEntry[],
  discoveredPlugins: DiscoveredChannelPlugin[],
): StartkitItemReport[] {
  if (choices.channels.length === 0) return reports;

  const rollup = reports.find((report) => report.id === "channels.plugins");
  const registryById = new Map(pluginRegistry.map((plugin) => [plugin.id, plugin]));
  const discoveredById = new Map(discoveredPlugins.map((plugin) => [plugin.id, plugin]));

  return choices.channels.map((channelId) => {
    const registryEntry = registryById.get(channelId);
    const discovered = discoveredById.get(channelId);
    const status = messagingPluginStatus(rollup, Boolean(discovered));
    return {
      id: `channels.plugins.${channelId}`,
      label: registryEntry?.name ?? discovered?.name ?? channelId,
      group: "messaging",
      category: "channels",
      status,
      severity: rollup?.severity,
      version: discovered?.version,
      path: discovered?.entry,
      message:
        registryEntry?.description ??
        rollup?.message ??
        "Selected messaging app",
      actions:
        status === "missing" || status === "outdated" || status === "broken"
          ? ["install"]
          : [],
      secret: false,
      settingsKey: undefined,
    };
  });
}

function messagingPluginStatus(
  rollup: StartkitItemReport | undefined,
  installed: boolean,
): StartkitStatus {
  if (rollup?.status === "running") return "running";
  if (rollup?.status === "ok") return "ok";
  if (rollup?.status === "error" || rollup?.status === "blocked") {
    return rollup.status;
  }
  return installed ? "ok" : "missing";
}

function isInstallStepReport(report: StartkitItemReport): boolean {
  return report.category !== "config" && report.status !== "needs_config";
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
  t: (key: string, params?: Record<string, string | number>) => string,
): string {
  switch (id) {
    case "agents":
      return choices.agents.length > 0
        ? t("{{count}} selected", { count: choices.agents.length })
        : t("Skipped");
    case "messaging":
      return choices.channels.length > 0
        ? t("{{count}} selected", { count: choices.channels.length })
        : t("Skipped");
    case "remote":
      return tunnelProvider === "none" ? t("Skipped") : tunnelProvider;
    default:
      return choices.source === "cn" ? t("China mirror") : t("Global source");
  }
}
