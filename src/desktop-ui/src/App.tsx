import { useCallback, useEffect, useRef, useState } from "react";
import {
  Activity,
  RefreshCw,
  Settings,
  Eye,
  Rocket,
} from "lucide-react";
import { useI18n } from "@va/i18n";
import { useChannelsState } from "./hooks/useChannelsState";
import { useTunnelsState } from "./hooks/useTunnelsState";
import { useAgentsRuntime } from "./hooks/useAgentsRuntime";
import { Button } from "@/components/ui/button";
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
import { StatusDashboard } from "./StatusDashboard";
import { SettingsDialog } from "./Settings";
import {
  getLauncherPreferences,
  rescanAgentEntries,
  rescanDesktopAppEntries,
  type LauncherPreferences,
} from "./Launch/api";
import { LanguageMenu } from "./components/LanguageMenu";
import { cn } from "./lib/utils";
import { UpdateIndicator } from "./UpdateIndicator";

// ---------------------------------------------------------------------------
// Routing + Dashboard
// ---------------------------------------------------------------------------

function App() {
  const [route, setRoute] = useState(() => window.location.pathname);
  const [startupScanDone, setStartupScanDone] = useState(false);

  useEffect(() => {
    const onPop = () => setRoute(window.location.pathname);
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);

  useEffect(() => {
    let cancelled = false;
    void Promise.allSettled([rescanAgentEntries(), rescanDesktopAppEntries()]).then(
      () => {
        if (!cancelled) setStartupScanDone(true);
      },
    );
    return () => {
      cancelled = true;
    };
  }, []);

  if (!startupScanDone) {
    return <Splash visible />;
  }

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
  const [launchRefreshToken, setLaunchRefreshToken] = useState(0);

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

  const handleRuntimeSettingsChanged = useCallback(() => {
    refreshAll();
    setLaunchRefreshToken((token) => token + 1);
  }, [refreshAll]);

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
        <div className="relative z-10 flex min-w-0 items-center gap-1.5 whitespace-nowrap">
          <VibeAroundMark />
          <span className="text-[13px] font-semibold text-foreground">
            VibeAround
          </span>
          <span className="font-mono text-[10px] text-muted-foreground/60">
            @{__APP_VERSION_LABEL__}
          </span>
          <UpdateIndicator />
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
              className={`size-4 ${
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
        onServicesRestarted={handleRuntimeSettingsChanged}
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
          <Launch refreshToken={launchRefreshToken} />
        </div>
      ) : (
        <StatusDashboard
          channels={channels}
          tunnels={tunnels}
          agents={agents}
        />
      )}
    </div>
  );
}

export default App;

function VibeAroundMark() {
  return (
    <img
      src="/brand/vibearound-mark.svg"
      alt=""
      className="h-5 w-5 shrink-0 rounded-md"
      aria-hidden="true"
      draggable={false}
    />
  );
}
