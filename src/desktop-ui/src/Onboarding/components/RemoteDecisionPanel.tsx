import { CheckCircle2, Download, Globe, Loader2 } from "lucide-react";
import { useI18n } from "@va/i18n";

import { cn } from "@/lib/utils";

import {
  compactReportLabel,
  tunnelDescription,
  tunnelRank,
} from "./startkitPresentation";
import type { StartkitItemReport, TunnelSummary } from "../types";
import type { TunnelProvider } from "../constants";

export function RemoteDecisionPanel({
  tunnels,
  provider,
  reports,
  onProvider,
}: {
  tunnels: TunnelSummary[];
  provider: TunnelProvider;
  reports: StartkitItemReport[];
  onProvider: (value: TunnelProvider) => void;
}) {
  const { t } = useI18n();
  const cloudflare = tunnels.find((tunnel) => tunnel.id === "cloudflare");
  const none = tunnels.find((tunnel) => tunnel.id === "none");
  const moreTunnels = tunnels
    .filter((tunnel) => tunnel.id !== "cloudflare" && tunnel.id !== "none")
    .sort((a, b) => tunnelRank(a.id) - tunnelRank(b.id));
  const visibleTunnels = [
    ...(none ? [none] : []),
    ...(cloudflare ? [cloudflare] : []),
    ...moreTunnels,
  ];

  return (
    <div className="mx-auto flex min-h-full w-full max-w-4xl items-center py-4">
      <section className="w-full space-y-3">
        <div className="px-1">
          <div className="flex items-center gap-2 text-base font-semibold">
            <Globe className="h-4 w-4 text-primary" />
            {t("Remote access")}
          </div>
          <p className="mt-1 text-sm text-muted-foreground">
            {t("Allow external access to this computer.")}
          </p>
        </div>

        <div className="grid gap-2 sm:grid-cols-2">
          {visibleTunnels.map((tunnel) => (
            <TunnelCard
              key={tunnel.id}
              tunnel={tunnel}
              selected={provider === tunnel.id}
              report={
                provider === tunnel.id
                  ? tunnelReportForProvider(tunnel.id, reports)
                  : undefined
              }
              recommended={tunnel.id === "cloudflare"}
              onSelect={() => onProvider(tunnel.id)}
              t={t}
            />
          ))}
        </div>
      </section>
    </div>
  );
}

function TunnelCard({
  tunnel,
  selected,
  report,
  recommended,
  onSelect,
  t,
}: {
  tunnel: TunnelSummary;
  selected: boolean;
  report?: StartkitItemReport;
  recommended?: boolean;
  onSelect: () => void;
  t: (key: string, params?: Record<string, string | number>) => string;
}) {
  return (
    <button
      type="button"
      className={cn(
        "min-h-[96px] rounded-md border p-4 text-left transition-colors",
        selected
          ? "border-primary/50 bg-primary/10"
          : "border-border bg-background hover:border-primary/30",
      )}
      onClick={onSelect}
    >
      <span className="flex items-center justify-between gap-2">
        <span className="text-sm font-medium">{tunnel.display_name}</span>
        {recommended && (
          <span className="rounded-full border border-primary/25 bg-primary/10 px-2 py-0.5 text-[10px] text-primary">
            {t("Recommended")}
          </span>
        )}
      </span>
      <span className="mt-2 block text-xs leading-5 text-muted-foreground">
        {tunnelDescription(tunnel.id, t)}
      </span>
      {report && (
        <span
          className={cn(
            "mt-2 inline-flex items-center gap-1.5 text-[11px]",
            report.status === "ok"
              ? "text-emerald-700 dark:text-emerald-300"
              : report.status === "outdated" || report.status === "running"
                ? "text-amber-700 dark:text-amber-300"
                : "text-muted-foreground",
          )}
        >
          {report.status === "running" ? (
            <Loader2 className="h-3 w-3 animate-spin" />
          ) : report.status === "ok" ? (
            <CheckCircle2 className="h-3 w-3" />
          ) : (
            <Download className="h-3 w-3" />
          )}
          {compactReportLabel(report, t)}
        </span>
      )}
    </button>
  );
}

function tunnelReportForProvider(
  provider: string,
  reports: StartkitItemReport[],
): StartkitItemReport | undefined {
  switch (provider) {
    case "cloudflare":
      return reports.find((report) => report.id === "tunnels.cloudflare.binary");
    case "localtunnel":
      return reports.find((report) =>
        report.id === "tunnels.localtunnel.package" ||
        report.id === "tunnels.localtunnel.system"
      );
    case "ngrok":
      return reports.find((report) => report.id === "tunnels.ngrok.binary");
    case "tailscale":
      return reports.find((report) => report.id === "tunnels.tailscale.binary");
    default:
      return undefined;
  }
}
