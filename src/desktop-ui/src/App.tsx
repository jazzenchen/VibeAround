import { useCallback, useEffect, useRef, useState } from "react";
import {
  Activity,
  Globe,
  Bot,
  MessageSquare,
  X,
  RefreshCw,
  ExternalLink,
  Settings,
  Eye,
  Play,
  Rocket,
} from "lucide-react";
import { useI18n } from "@va/i18n";
import type { TunnelStatus } from "@va/client";
import {
  useChannelsState,
  type ChannelRuntime,
} from "./hooks/useChannelsState";
import { useTunnelsState, type TunnelRuntime } from "./hooks/useTunnelsState";
import { useAgentsRuntime, type AgentRuntime } from "./hooks/useAgentsRuntime";
import { openDashboardUrl, DAEMON_PORT } from "./lib/api";
import { Button } from "@/components/ui/button";
import { PageHeader, PageShell, SectionCard } from "@/components/page";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { Splash } from "./Splash";
import Onboarding from "./Onboarding";
import { Previews } from "./Previews";
import { Launch } from "./Launch";
import { SettingsDialog } from "./Settings";
import { getLauncherPreferences, type LauncherPreferences } from "./Launch/api";
import { LanguageMenu } from "./components/LanguageMenu";
import { cn } from "./lib/utils";

// ---------------------------------------------------------------------------
// Per-domain status presentation — each manager has its own natural status
// shape (channel: string enum; tunnel: TunnelStatus; agent: derived
// from busy/failed flags), so each gets its own mapping.
// ---------------------------------------------------------------------------

type Pres = { label: string; color: string; running: boolean };
type Translate = ReturnType<typeof useI18n>["t"];

function channelStatusPresentation(
  status: ChannelRuntime["status"],
  t: Translate,
): Pres {
  switch (status) {
    case "running":
      return { label: t("Running"), color: "bg-emerald-500", running: true };
    case "spawning":
      return { label: t("Spawning"), color: "bg-amber-500", running: false };
    case "not_started":
      return { label: t("Not started"), color: "bg-zinc-400", running: false };
    case "stopped":
      return { label: t("Stopped"), color: "bg-zinc-400", running: false };
    case "crashed":
      return { label: t("Crashed"), color: "bg-red-500", running: false };
  }
}

function tunnelStatusPresentation(status: TunnelStatus, t: Translate): Pres {
  switch (status.state) {
    case "running":
      return { label: t("Running"), color: "bg-emerald-500", running: true };
    case "stopped":
      return { label: t("Stopped"), color: "bg-zinc-400", running: false };
    case "failed":
      return { label: t("Failed"), color: "bg-red-500", running: false };
  }
}

function agentStatusPresentation(agent: AgentRuntime, t: Translate): Pres {
  if (agent.failed)
    return { label: t("Failed"), color: "bg-red-500", running: false };
  if (agent.busy)
    return { label: t("Busy"), color: "bg-amber-500", running: true };
  return { label: t("Idle"), color: "bg-emerald-500", running: true };
}

function StatusDot({ colorClass }: { colorClass: string }) {
  return <span className={`inline-block w-2 h-2 rounded-full ${colorClass}`} />;
}

// ---------------------------------------------------------------------------
// Per-domain row components
// ---------------------------------------------------------------------------

function ChannelRow({
  channel,
  onStart,
  onStop,
  t,
}: {
  channel: ChannelRuntime;
  onStart: () => void;
  onStop: () => void;
  t: Translate;
}) {
  const pres = channelStatusPresentation(channel.status, t);
  const showRestartIn =
    channel.status === "crashed" && channel.restart_in_secs > 0;
  return (
    <Row
      dot={pres.color}
      name={capitalize(channel.kind)}
      label={pres.label}
      running={pres.running}
      title={channel.reason ?? pres.label}
      suffix={
        showRestartIn
          ? ` · ${t("retry {{seconds}}s", { seconds: channel.restart_in_secs })}`
          : null
      }
      actions={
        <>
          {!pres.running && (
            <IconBtn
              onClick={onStart}
              title={t("Start")}
              icon={<Play className="w-3 h-3" />}
              hover="emerald"
            />
          )}
          {pres.running && (
            <IconBtn
              onClick={onStop}
              title={t("Stop")}
              icon={<X className="w-3 h-3" />}
              hover="destructive"
            />
          )}
        </>
      }
    />
  );
}

function TunnelRow({
  tunnel,
  onKill,
  t,
}: {
  tunnel: TunnelRuntime;
  onKill: () => void;
  t: Translate;
}) {
  const pres = tunnelStatusPresentation(tunnel.status, t);
  const tooltip =
    tunnel.status.state === "stopped"
      ? (tunnel.status.reason ?? pres.label)
      : tunnel.status.state === "failed"
        ? tunnel.status.error
        : pres.label;
  return (
    <Row
      dot={pres.color}
      name={t("Tunnel ({{provider}})", { provider: tunnel.provider })}
      label={pres.label}
      running={pres.running}
      title={tooltip}
      secondary={tunnel.provider}
      tailLink={tunnel.url ? { url: tunnel.url } : undefined}
      actions={
        pres.running ? (
          <IconBtn
            onClick={onKill}
            title={t("Stop")}
            icon={<X className="w-3 h-3" />}
            hover="destructive"
          />
        ) : null
      }
    />
  );
}

function AgentRow({
  agent,
  onKill,
  t,
}: {
  agent: AgentRuntime;
  onKill: () => void;
  t: Translate;
}) {
  const pres = agentStatusPresentation(agent, t);
  const kindLabel = agent.cli_kind ?? "agent";
  const name = `${kindLabel} (${agent.route_key})`;
  return (
    <Row
      dot={pres.color}
      name={name}
      label={pres.label}
      running={pres.running}
      title={agent.failed ?? agent.session_id ?? pres.label}
      secondary={agent.agent_version ? `v${agent.agent_version}` : undefined}
      actions={
        pres.running ? (
          <IconBtn
            onClick={onKill}
            title={t("Stop")}
            icon={<X className="w-3 h-3" />}
            hover="destructive"
          />
        ) : null
      }
    />
  );
}

function Row({
  dot,
  name,
  label,
  running,
  title,
  suffix,
  secondary,
  tailLink,
  actions,
}: {
  dot: string;
  name: string;
  label: string;
  running: boolean;
  title: string;
  suffix?: React.ReactNode;
  secondary?: string;
  tailLink?: { url: string };
  actions?: React.ReactNode;
}) {
  return (
    <div className="flex items-center gap-1.5 py-1 px-2 rounded-md hover:bg-accent/50 transition-colors group">
      <StatusDot colorClass={dot} />
      <span className="text-xs font-medium flex-1 truncate">{name}</span>
      {secondary && (
        <span className="text-[10px] text-muted-foreground/70 truncate max-w-[100px]">
          {secondary}
        </span>
      )}
      <span
        className={`text-[10px] tabular-nums ${running ? "text-muted-foreground/60" : "text-muted-foreground/80"}`}
        title={title}
      >
        {label}
        {suffix && <span className="text-muted-foreground/50">{suffix}</span>}
      </span>
      {tailLink && (
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          onClick={(e) => {
            e.preventDefault();
            void openDashboardUrl(tailLink.url);
          }}
          className="text-muted-foreground/50 hover:text-primary"
          title={tailLink.url}
        >
          <ExternalLink className="w-3 h-3" />
        </Button>
      )}
      {actions}
    </div>
  );
}

function IconBtn({
  onClick,
  title,
  icon,
  hover,
}: {
  onClick: () => void;
  title: string;
  icon: React.ReactNode;
  hover: "destructive" | "emerald";
}) {
  const hoverClass =
    hover === "destructive"
      ? "hover:text-destructive"
      : "hover:text-emerald-500";
  return (
    <Button
      type="button"
      variant="ghost"
      size="icon-xs"
      onClick={onClick}
      className={`text-muted-foreground/40 ${hoverClass} opacity-0 group-hover:opacity-100 transition-opacity`}
      title={title}
    >
      {icon}
    </Button>
  );
}

function capitalize(s: string): string {
  return s.length === 0 ? s : s[0].toUpperCase() + s.slice(1);
}

// ---------------------------------------------------------------------------
// Routing + Dashboard
// ---------------------------------------------------------------------------

function App() {
  const [route, setRoute] = useState(() => window.location.pathname);

  useEffect(() => {
    const onPop = () => setRoute(window.location.pathname);
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);

  if (route === "/onboarding") {
    return <Onboarding />;
  }

  return <Dashboard />;
}

type DashboardPage = "launch" | "status" | "previews";

function Dashboard() {
  const { t } = useI18n();
  const isMacTitlebar =
    typeof navigator !== "undefined" && /Mac/.test(navigator.platform);
  const [page, setPage] = useState<DashboardPage>("launch");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [launcherPrefs, setLauncherPrefs] =
    useState<LauncherPreferences | null>(null);
  const [launcherPrefsLoaded, setLauncherPrefsLoaded] = useState(false);

  const channels = useChannelsState();
  const tunnels = useTunnelsState();
  const agents = useAgentsRuntime();

  const anyEverLoaded =
    channels.everLoaded || tunnels.everLoaded || agents.everLoaded;
  const firstError = channels.error ?? tunnels.error ?? agents.error ?? null;

  const refreshLauncherPrefs = useCallback(() => {
    void getLauncherPreferences()
      .then((prefs) => {
        setLauncherPrefs(prefs);
        setLauncherPrefsLoaded(true);
      })
      .catch(() => {
        setLauncherPrefs(null);
        setLauncherPrefsLoaded(true);
      });
  }, []);

  const refreshAll = useCallback(() => {
    void channels.refresh();
    void tunnels.refresh();
    void agents.refresh();
    refreshLauncherPrefs();
  }, [channels, tunnels, agents, refreshLauncherPrefs]);

  const everHadData = useRef(false);
  const [startTime] = useState(() => Date.now());
  const [timedOut, setTimedOut] = useState(false);
  const launchEnabled = !launcherPrefsLoaded
    ? false
    : launcherPrefs
      ? launcherPrefs.enabledAgents.length > 0
      : true;
  const launchDisabledReason = !launcherPrefsLoaded
    ? t("Loading launch settings")
    : !launchEnabled
      ? t("No launch agents enabled")
      : null;
  const effectivePage = !launchEnabled && page === "launch" ? "status" : page;

  if (anyEverLoaded) everHadData.current = true;

  useEffect(() => {
    refreshLauncherPrefs();
  }, [refreshLauncherPrefs]);

  useEffect(() => {
    if (launcherPrefsLoaded && !launchEnabled && page === "launch") {
      setPage("status");
    }
  }, [launchEnabled, launcherPrefsLoaded, page]);

  useEffect(() => {
    if (anyEverLoaded || everHadData.current) return;
    if (timedOut) return;
    const elapsed = Date.now() - startTime;
    if (elapsed > 30_000) {
      setTimedOut(true);
      return;
    }
    if (firstError) {
      const timer = setTimeout(refreshAll, 2000);
      return () => clearTimeout(timer);
    }
  }, [anyEverLoaded, firstError, timedOut, startTime, refreshAll]);

  const showSplash = !everHadData.current && !anyEverLoaded && !timedOut;
  if (showSplash) return <Splash visible />;

  if (timedOut && !anyEverLoaded) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-3">
        <p className="text-xs text-destructive">
          {t("Server failed to start")}
        </p>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={() => {
            setTimedOut(false);
            refreshAll();
          }}
          className="text-primary hover:text-primary"
        >
          <RefreshCw className="w-3 h-3" /> {t("Retry")}
        </Button>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col">
      <header
        className={cn(
          "relative flex h-12 items-center justify-between pr-3 border-b border-border shrink-0",
          isMacTitlebar ? "pl-[82px]" : "pl-3",
        )}
      >
        <div
          data-tauri-drag-region
          aria-hidden="true"
          className="absolute inset-0 z-0"
        />
        <div className="relative z-10 flex min-w-0 items-baseline gap-1.5 whitespace-nowrap">
          <span className="text-[13px] font-semibold text-foreground">
            VibeAround
          </span>
          <span className="font-mono text-[10px] text-muted-foreground/60">
            @{__APP_VERSION_LABEL__}
          </span>
        </div>
        <div className="absolute left-1/2 top-1/2 z-10 -translate-x-1/2 -translate-y-1/2">
          <Tabs
            value={effectivePage}
            onValueChange={(value) => {
              if (value === "launch" && !launchEnabled) return;
              setPage(value as DashboardPage);
            }}
            className="contents"
          >
            <TabsList className="!h-8 rounded-md p-1">
              <TooltipProvider>
                {launchDisabledReason ? (
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <span
                        className="inline-flex cursor-not-allowed"
                        tabIndex={0}
                        role="button"
                        aria-disabled="true"
                        aria-label={launchDisabledReason}
                        title={launchDisabledReason}
                      >
                        <TabsTrigger
                          value="launch"
                          disabled
                          className="!h-6 gap-1 px-2 text-xs [&_svg:not([class*='size-'])]:!size-3.5"
                        >
                          <Rocket /> {t("Launch")}
                        </TabsTrigger>
                      </span>
                    </TooltipTrigger>
                    <TooltipContent side="bottom">
                      {launchDisabledReason}
                    </TooltipContent>
                  </Tooltip>
                ) : (
                  <TabsTrigger
                    value="launch"
                    className="!h-6 gap-1 px-2 text-xs [&_svg:not([class*='size-'])]:!size-3.5"
                  >
                    <Rocket /> {t("Launch")}
                  </TabsTrigger>
                )}
              </TooltipProvider>
              <TabsTrigger
                value="status"
                className="!h-6 gap-1 px-2 text-xs [&_svg:not([class*='size-'])]:!size-3.5"
              >
                <Activity /> {t("Status")}
              </TabsTrigger>
              <TabsTrigger
                value="previews"
                className="!h-6 gap-1 px-2 text-xs [&_svg:not([class*='size-'])]:!size-3.5"
              >
                <Eye /> {t("Previews")}
              </TabsTrigger>
            </TabsList>
          </Tabs>
        </div>
        <div className="relative z-10 flex items-center gap-2">
          <LanguageMenu />
          <Button
            onClick={() => setSettingsOpen(true)}
            variant="ghost"
            size="icon-xs"
            title={t("Settings")}
            aria-label={t("Settings")}
            className={
              settingsOpen
                ? "bg-accent text-accent-foreground"
                : undefined
            }
          >
            <Settings
              className={`w-3.5 h-3.5 ${
                settingsOpen
                  ? "text-accent-foreground"
                  : "text-muted-foreground"
              }`}
            />
          </Button>
        </div>
      </header>

      <SettingsDialog
        open={settingsOpen}
        onOpenChange={setSettingsOpen}
        onServicesRestarted={refreshAll}
      />

      {firstError && (
        <div className="px-3 py-1 bg-destructive/10 text-destructive text-xs">
          {firstError}
        </div>
      )}

      {effectivePage === "previews" ? (
        <div className="flex-1 overflow-y-auto">
          <Previews />
        </div>
      ) : effectivePage === "launch" ? (
        <div className="flex-1 min-h-0">
          <Launch />
        </div>
      ) : (
        <div className="flex-1 overflow-y-auto">
          <PageShell className="space-y-3">
            <PageHeader
              icon={<Activity className="w-4 h-4 text-primary" />}
              title={t("Status")}
              description={t(
                "Runtime health for tunnels, agents, and messaging channels.",
              )}
              actions={
                <Button
                  type="button"
                  variant="ghost"
                  size="xs"
                  className="text-primary hover:text-primary"
                  onClick={(e) => {
                    e.preventDefault();
                    void openDashboardUrl(
                      `http://127.0.0.1:${DAEMON_PORT}/va/`,
                    );
                  }}
                >
                  {t("Open Web Dashboard")} <ExternalLink className="w-3 h-3" />
                </Button>
              }
            />

            <SectionCard
              icon={<Globe className="w-4 h-4 text-primary" />}
              title={t("Tunnel")}
              badge={tunnels.tunnels.length}
            >
              {tunnels.tunnels.length === 0 ? (
                <p className="text-xs text-muted-foreground px-3 py-2">
                  {t("No tunnel running")}
                </p>
              ) : (
                tunnels.tunnels.map((tunnel) => (
                  <TunnelRow
                    key={tunnel.provider}
                    tunnel={tunnel}
                    onKill={() => tunnels.kill(tunnel.provider)}
                    t={t}
                  />
                ))
              )}
            </SectionCard>

            <SectionCard
              icon={<Bot className="w-4 h-4 text-primary" />}
              title={t("Agents")}
              badge={agents.agents.length}
            >
              {agents.agents.length === 0 ? (
                <p className="text-xs text-muted-foreground px-3 py-2">
                  {t("No agents running")}
                </p>
              ) : (
                agents.agents.map((a) => (
                  <AgentRow
                    key={a.route_key}
                    agent={a}
                    onKill={() => agents.kill(a.route_key)}
                    t={t}
                  />
                ))
              )}
            </SectionCard>

            <SectionCard
              icon={<MessageSquare className="w-4 h-4 text-primary" />}
              title={t("Channels")}
              badge={channels.channels.length}
            >
              {channels.channels.length === 0 ? (
                <p className="text-xs text-muted-foreground px-3 py-2">
                  {t("No channels running")}
                </p>
              ) : (
                channels.channels.map((c) => (
                  <ChannelRow
                    key={c.kind}
                    channel={c}
                    onStart={() => channels.start(c.kind)}
                    onStop={() => channels.stop(c.kind)}
                    t={t}
                  />
                ))
              )}
            </SectionCard>
          </PageShell>
        </div>
      )}
    </div>
  );
}

export default App;
