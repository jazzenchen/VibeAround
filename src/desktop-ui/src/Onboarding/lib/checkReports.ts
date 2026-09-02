import { groupReports } from "../components/startkitPresentation";
import type {
  AgentSummary,
  DiscoveredChannelPlugin,
  PluginRegistryEntry,
  StartkitItemReport,
  TunnelSummary,
} from "../types";

export function mergeReportsById(
  previous: StartkitItemReport[],
  incoming: StartkitItemReport[],
): StartkitItemReport[] {
  const merged = new Map(previous.map((report) => [report.id, report]));
  for (const report of incoming) merged.set(report.id, report);
  return Array.from(merged.values());
}

export function mergeLocalReportsById(
  previous: StartkitItemReport[],
  incoming: StartkitItemReport[],
): StartkitItemReport[] {
  const merged = new Map(previous.map((report) => [report.id, report]));
  for (const report of incoming) {
    const existing = merged.get(report.id);
    if (existing?.status === "running") continue;
    if (existing?.latestVersion && existing.version === report.version) continue;
    merged.set(report.id, report);
  }
  return Array.from(merged.values());
}

export function markReportsUpdating(
  reports: StartkitItemReport[],
  reportIds: Set<string>,
  message: string,
): StartkitItemReport[] {
  return reports.map((report) =>
    reportIds.has(report.id) &&
    (report.status === "ok" ||
      report.status === "missing" ||
      report.status === "outdated")
      ? { ...report, message }
      : report,
  );
}

export function hasSkippedReport(
  reports: StartkitItemReport[],
  reportId: string,
): boolean {
  return reports.some(
    (report) => report.id === reportId && report.status === "skipped",
  );
}

export function agentIdFromReport(report: StartkitItemReport): string | null {
  const match = /^agents\.(.+)\.cli$/.exec(report.id);
  return match?.[1] ?? null;
}

export function pluginIdFromReport(report: StartkitItemReport): string | null {
  const match = /^channels\.plugins\.(.+)$/.exec(report.id);
  return match?.[1] ?? null;
}

export function tunnelReportMatchesProvider(
  report: StartkitItemReport,
  provider: string,
): boolean {
  switch (provider) {
    case "cloudflare":
      return report.id === "tunnels.cloudflare.binary";
    case "localtunnel":
      return (
        report.id === "tunnels.localtunnel.package" ||
        report.id === "tunnels.localtunnel.system"
      );
    case "ngrok":
      return report.id === "tunnels.ngrok.binary";
    case "tailscale":
      return report.id === "tunnels.tailscale.binary";
    default:
      return false;
  }
}

export function groupReportsFromReports(reports: StartkitItemReport[]) {
  const reportById = new Map(reports.map((report) => [report.id, report]));
  return groupReports(
    reports.map((report) => ({
      id: report.id,
      group: report.group,
      label: report.label,
      category: report.category,
      severity: report.severity,
      secret: report.secret,
      settingsKey: report.settingsKey,
    })),
    reportById,
  );
}

export function itemCheckSignature(id: string, ...parts: string[]): string {
  return JSON.stringify([id, ...parts]);
}

export function agentCheckingReport(
  agentId: string,
  agents: AgentSummary[],
  message: string,
): StartkitItemReport {
  const agent = agents.find((item) => item.id === agentId);
  return {
    id: `agents.${agentId}.cli`,
    label: agent?.display_name ?? agentId,
    group: "agents",
    category: "agents",
    status: "running",
    message,
    actions: [],
    secret: false,
  };
}

export function localPluginReport(
  entry: PluginRegistryEntry,
  discoveredPlugins: DiscoveredChannelPlugin[],
): StartkitItemReport {
  const discovered = discoveredPlugins.find((plugin) => plugin.id === entry.id);
  return {
    id: `channels.plugins.${entry.id}`,
    label: entry.name,
    group: "messaging",
    category: "channels",
    status: discovered ? "ok" : "missing",
    version: discovered?.version,
    path: discovered?.entry,
    message: discovered ? "Plugin is installed" : "Plugin is not installed",
    actions: discovered ? [] : ["install"],
    secret: false,
  };
}

export function pluginCheckingReport(
  pluginId: string,
  registry: PluginRegistryEntry[],
  discoveredPlugins: DiscoveredChannelPlugin[],
): StartkitItemReport {
  const entry = registry.find((plugin) => plugin.id === pluginId);
  const discovered = discoveredPlugins.find((plugin) => plugin.id === pluginId);
  return {
    id: `channels.plugins.${pluginId}`,
    label: entry?.name ?? discovered?.name ?? pluginId,
    group: "messaging",
    category: "channels",
    status: "running",
    version: discovered?.version,
    path: discovered?.entry,
    message: "Checking updates",
    actions: [],
    secret: false,
  };
}

export function tunnelCheckingReport(
  tunnelId: string,
  tunnels: TunnelSummary[],
): StartkitItemReport {
  const tunnel = tunnels.find((item) => item.id === tunnelId);
  if (tunnelId === "localtunnel") {
    return {
      id: "tunnels.localtunnel.package",
      label: tunnel?.display_name ?? "localtunnel",
      group: "remote",
      category: "tunnels",
      status: "running",
      message: "Checking local version",
      actions: [],
      secret: false,
    };
  }
  return {
    id: `tunnels.${tunnelId}.binary`,
    label: tunnel?.display_name ?? tunnelId,
    group: "remote",
    category: "tunnels",
    status: "running",
    message: "Checking local version",
    actions: [],
    secret: false,
  };
}
