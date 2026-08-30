import { useCallback, useEffect, useMemo, useState } from "react";
import { Loader2, Server } from "lucide-react";
import { useI18n } from "@va/i18n";

import { BrandIcon } from "@/components/brand-icon";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";
import {
  getLauncherPreferences,
  listAgents,
  listProfiles,
  setLauncherLocalAgentApiAgentEnabled,
  setLauncherLocalAgentApiEnabled,
  type AgentSummary,
  type LauncherPreferences,
} from "./Launch/api";
import {
  localAgentBasePath,
  type LocalAgentApiTarget,
} from "./Launch/localAgentApi";
import { LocalAgentApiPanel } from "./Launch/LocalAgentApiPanel";
import {
  agentWorkspace,
  isBridgeAgent,
  profileSupportsAgent,
} from "./Launch/launchModel";
import type { ProfileSummary } from "./Launch/types";

interface LocalApiRoute {
  id: string;
  agentId: string;
  agentLabel: string;
  profileId: string;
  profileLabel: string;
  providerId: string | null;
  providerLabel: string;
  providerIcon: string | null;
  workspacePath: string;
  target: LocalAgentApiTarget;
}

export function LocalApiWorkbench({
  refreshToken = 0,
}: {
  refreshToken?: number;
}) {
  const { t } = useI18n();
  const [agents, setAgents] = useState<AgentSummary[]>([]);
  const [profiles, setProfiles] = useState<ProfileSummary[]>([]);
  const [prefs, setPrefs] = useState<LauncherPreferences | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [agentId, setAgentId] = useState("");
  const [routeId, setRouteId] = useState("");
  const [serviceSaving, setServiceSaving] = useState(false);
  const [agentApiSaving, setAgentApiSaving] = useState(false);

  const refresh = useCallback(async () => {
    setError(null);
    try {
      const [nextAgents, nextProfiles, nextPrefs] = await Promise.all([
        listAgents(),
        listProfiles(),
        getLauncherPreferences(),
      ]);
      setAgents(nextAgents.filter((agent) => isBridgeAgent(agent.id)));
      setProfiles(nextProfiles);
      setPrefs(nextPrefs);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh, refreshToken]);

  const visibleAgents = useMemo(() => {
    if (!prefs) return agents;
    const enabled = new Set(prefs.enabledAgents);
    return agents.filter((agent) => enabled.has(agent.id));
  }, [agents, prefs]);

  useEffect(() => {
    if (!prefs || visibleAgents.length === 0) return;
    if (agentId && visibleAgents.some((agent) => agent.id === agentId)) return;
    const preferred = visibleAgents.find(
      (agent) => agent.id === prefs.selectedAgent,
    );
    setAgentId((preferred ?? visibleAgents[0]).id);
  }, [agentId, prefs, visibleAgents]);

  const selectedAgent =
    visibleAgents.find((agent) => agent.id === agentId) ??
    visibleAgents[0] ??
    null;
  const serviceEnabled = prefs?.localAgentApiEnabled ?? false;

  const routes = useMemo<LocalApiRoute[]>(() => {
    if (!selectedAgent || !prefs) return [];
    const workspacePath = agentWorkspace(prefs, selectedAgent.id);
    const directTarget: LocalAgentApiTarget = {
      agentId: selectedAgent.id,
      agentLabel: selectedAgent.display_name,
      profileId: "direct",
      profileLabel: t("Direct"),
      workspacePath,
    };
    const allRoutes: LocalApiRoute[] = [
      {
        id: `${selectedAgent.id}:direct`,
        agentId: selectedAgent.id,
        agentLabel: selectedAgent.display_name,
        profileId: "direct",
        profileLabel: t("Direct"),
        providerId: null,
        providerLabel: t("Use existing CLI login"),
        providerIcon: null,
        workspacePath,
        target: directTarget,
      },
      ...profiles
        .filter((profile) =>
          profileSupportsAgent(profile, selectedAgent.id, prefs),
        )
        .map((profile) => {
          const target: LocalAgentApiTarget = {
            agentId: selectedAgent.id,
            agentLabel: selectedAgent.display_name,
            profileId: profile.id,
            profileLabel: profile.label,
            workspacePath,
          };
          return {
            id: `${selectedAgent.id}:${profile.id}`,
            agentId: selectedAgent.id,
            agentLabel: selectedAgent.display_name,
            profileId: profile.id,
            profileLabel: profile.label,
            providerId: profile.provider,
            providerLabel: profile.providerLabel,
            providerIcon: profile.providerIcon,
            workspacePath,
            target,
          };
        }),
    ];
    return allRoutes;
  }, [profiles, prefs, selectedAgent, t]);

  useEffect(() => {
    if (routes.length === 0) {
      setRouteId("");
      return;
    }
    if (routeId && routes.some((route) => route.id === routeId)) return;
    setRouteId(routes[0].id);
  }, [routeId, routes]);

  const selectedRoute =
    routes.find((route) => route.id === routeId) ?? routes[0] ?? null;

  async function handleServiceToggle(nextEnabled: boolean) {
    if (!prefs || serviceSaving) return;
    setServiceSaving(true);
    setError(null);
    const previous = prefs.localAgentApiEnabled;
    setPrefs({ ...prefs, localAgentApiEnabled: nextEnabled });
    try {
      await setLauncherLocalAgentApiEnabled(nextEnabled);
      await refresh();
    } catch (err) {
      setPrefs({ ...prefs, localAgentApiEnabled: previous });
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setServiceSaving(false);
    }
  }

  const agentApiAgents = prefs?.localAgentApiAgents ?? [];
  const agentApiEnabled =
    !!selectedAgent && agentApiAgents.includes(selectedAgent.id);

  async function handleAgentApiToggle(nextEnabled: boolean) {
    if (!prefs || !selectedAgent || agentApiSaving) return;
    setAgentApiSaving(true);
    setError(null);
    const previous = prefs.localAgentApiAgents ?? [];
    const next = nextEnabled
      ? [...previous.filter((id) => id !== selectedAgent.id), selectedAgent.id]
      : previous.filter((id) => id !== selectedAgent.id);
    setPrefs({ ...prefs, localAgentApiAgents: next });
    try {
      await setLauncherLocalAgentApiAgentEnabled(selectedAgent.id, nextEnabled);
      await refresh();
    } catch (err) {
      setPrefs({ ...prefs, localAgentApiAgents: previous });
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setAgentApiSaving(false);
    }
  }

  if (loading) {
    return <p className="p-3 text-xs text-muted-foreground">{t("Loading…")}</p>;
  }

  if (error) {
    return (
      <div className="p-3 text-xs text-destructive" role="alert">
        {error}
      </div>
    );
  }

  if (!selectedAgent || !prefs) {
    return (
      <div className="flex h-full items-center justify-center text-xs text-muted-foreground">
        {t("No local API agents enabled")}
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col bg-muted/15">
      <header className="flex h-12 shrink-0 items-center justify-between border-b border-border bg-background px-4">
        <div className="flex min-w-0 items-center gap-2">
          <Server className="h-4 w-4 text-primary" />
          <span className="shrink-0 font-semibold">
            {t("Local API service")}
          </span>
          <span className="min-w-0 truncate text-[11px] text-muted-foreground">
            {t("Expose local agents as OpenAI / Anthropic compatible APIs.")}
          </span>
          <Badge
            className={cn(
              "h-5 shrink-0 rounded-md px-2 text-[11px]",
              serviceEnabled
                ? "bg-primary/10 text-primary"
                : "bg-muted text-muted-foreground",
            )}
          >
            {serviceEnabled ? t("Running") : t("Stopped")}
          </Badge>
        </div>
        <div className="flex shrink-0 items-center gap-3 text-[11px] text-muted-foreground">
          <label className="flex items-center gap-2">
            <span>
              {serviceEnabled ? t("Service enabled") : t("Service disabled")}
            </span>
            {serviceSaving && <Loader2 className="h-3 w-3 animate-spin" />}
            <Switch
              checked={serviceEnabled}
              disabled={serviceSaving}
              onCheckedChange={(checked) => void handleServiceToggle(checked)}
              aria-label={t("Toggle local API service")}
            />
          </label>
        </div>
      </header>

      <div
        className={cn(
          "grid min-h-0 flex-1 grid-cols-[320px_minmax(0,1fr)] transition-[filter,opacity]",
          !serviceEnabled && "pointer-events-none opacity-45 grayscale",
        )}
      >
        <aside className="flex min-h-0 flex-col border-r border-border bg-background/70">
          <div className="shrink-0 border-b border-border px-4 py-3">
            <div className="flex items-center gap-2">
              <BrandIcon
                kind="cli"
                id={selectedAgent.id}
                label={selectedAgent.display_name}
                className="h-5 w-5"
              />
              <div className="min-w-0 flex-1">
                <div className="truncate text-sm font-semibold">
                  {selectedAgent.display_name}
                </div>
              </div>
            </div>
            <label className="mt-2 flex items-center justify-between gap-2 text-[11px] text-muted-foreground">
              <span>
                {agentApiEnabled
                  ? t("Agent API enabled")
                  : t("Agent API disabled")}
              </span>
              <span className="flex items-center gap-2">
                {agentApiSaving && <Loader2 className="h-3 w-3 animate-spin" />}
                <Switch
                  checked={agentApiEnabled}
                  disabled={agentApiSaving || !serviceEnabled}
                  onCheckedChange={(checked) =>
                    void handleAgentApiToggle(checked)
                  }
                  aria-label={t("Toggle API access for this agent")}
                />
              </span>
            </label>
            <div className="mt-3 flex flex-wrap gap-1.5 pb-0.5">
              {visibleAgents.map((agent) => (
                <button
                  key={agent.id}
                  type="button"
                  className={cn(
                    "inline-flex h-7 min-w-0 cursor-pointer items-center gap-1.5 rounded-md border px-2 text-[11px]",
                    agent.id === selectedAgent.id
                      ? "border-primary bg-primary/10 text-primary"
                      : "border-border bg-background text-muted-foreground hover:bg-accent/45",
                  )}
                  onClick={() => {
                    setAgentId(agent.id);
                    setRouteId("");
                  }}
                >
                  <BrandIcon
                    kind="cli"
                    id={agent.id}
                    label={agent.display_name}
                    className="h-4 w-4"
                  />
                  <span className="truncate">{agent.display_name}</span>
                </button>
              ))}
            </div>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto px-3 py-3">
            <RouteSection
              title={t("Routes")}
              routes={routes}
              selectedRouteId={selectedRoute?.id ?? ""}
              onSelect={setRouteId}
            />
          </div>
        </aside>

        <main className="min-h-0 overflow-y-auto px-6 py-5">
          {selectedRoute && (
            <div className="mx-auto grid max-w-[960px] gap-4">
              <section className="flex items-start justify-between gap-4">
                <div className="flex min-w-0 items-start gap-3">
                  <BrandIcon
                    kind={
                      selectedRoute.profileId === "direct" ? "cli" : "provider"
                    }
                    id={
                      selectedRoute.profileId === "direct"
                        ? selectedRoute.agentId
                        : (selectedRoute.providerId ??
                          selectedRoute.providerLabel)
                    }
                    fallback={selectedRoute.providerIcon}
                    label={selectedRoute.profileLabel}
                    className="h-10 w-10"
                  />
                  <div className="min-w-0">
                    <div className="flex flex-wrap items-center gap-2">
                      <h1 className="truncate text-xl font-semibold">
                        {selectedRoute.profileLabel}
                      </h1>
                      {selectedRoute.profileId === "direct" && (
                        <Badge
                          variant="outline"
                          className="h-5 rounded-md border-amber-300 bg-amber-50 px-1.5 text-[11px] text-amber-700"
                        >
                          {t("Default")}
                        </Badge>
                      )}
                    </div>
                    <p className="mt-1 truncate text-xs text-muted-foreground">
                      {selectedRoute.providerLabel}
                    </p>
                  </div>
                </div>
                <Badge
                  className={cn(
                    "h-6 rounded-md px-2 text-[11px]",
                    serviceEnabled
                      ? "bg-primary/10 text-primary"
                      : "bg-muted text-muted-foreground",
                  )}
                >
                  {serviceEnabled ? t("Bridge enabled") : t("Bridge disabled")}
                </Badge>
              </section>

              <LocalAgentApiPanel
                key={selectedRoute.id}
                target={selectedRoute.target}
                serviceEnabled={serviceEnabled}
              />
            </div>
          )}
        </main>
      </div>
    </div>
  );
}

function RouteSection({
  title,
  routes,
  selectedRouteId,
  onSelect,
}: {
  title: string;
  routes: LocalApiRoute[];
  selectedRouteId: string;
  onSelect: (routeId: string) => void;
}) {
  if (routes.length === 0) return null;
  return (
    <section className="mb-4">
      <div className="mb-2 px-1 text-[11px] font-medium text-muted-foreground">
        {title}
      </div>
      <div className="grid gap-1.5">
        {routes.map((route) => (
          <button
            key={route.id}
            type="button"
            className={cn(
              "flex min-h-[46px] w-full cursor-pointer items-center gap-2 rounded-md border px-2 text-left transition-colors",
              route.id === selectedRouteId
                ? "border-primary bg-card shadow-[inset_3px_0_0_hsl(var(--primary))]"
                : "border-transparent hover:border-border hover:bg-card",
            )}
            onClick={() => onSelect(route.id)}
          >
            <BrandIcon
              kind={route.profileId === "direct" ? "cli" : "provider"}
              id={
                route.profileId === "direct"
                  ? route.agentId
                  : (route.providerId ?? route.providerLabel)
              }
              fallback={route.providerIcon}
              label={route.profileLabel}
              className="h-7 w-7"
            />
            <span className="min-w-0 flex-1">
              <span className="block truncate text-xs font-semibold">
                {route.profileLabel}
              </span>
              <span className="block truncate font-mono text-[10px] text-muted-foreground">
                {localAgentBasePath(route.target).replace("/local-agent/", "/")}
              </span>
            </span>
          </button>
        ))}
      </div>
    </section>
  );
}
