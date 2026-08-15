import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import { Menu } from "lucide-react";
import type { WebVerboseSettings } from "@va/client";
import { useI18n } from "@va/i18n";

import { AppHeader } from "@/components/AppHeader";
import { ChatView } from "@/components/chat";
import { TabBar } from "@/components/TabBar";
import { Button } from "@/components/ui/button";
import { useSessions } from "@/hooks/useSessions";
import { useTmux } from "@/hooks/useTmux";
import type { AppPage } from "@/lib/dashboard-types";
import type { ViewMode } from "@/lib/terminal-types";
import { cn } from "@/lib/utils";
import { ThemeContext, getResolvedTheme, toggleTheme as applyThemeToggle, type Theme } from "@/lib/theme";

function workspacePaneClass(active: boolean) {
  return cn(
    "absolute inset-0 min-h-0 transition-opacity duration-150",
    active ? "z-10 opacity-100 pointer-events-auto" : "z-0 opacity-0 pointer-events-none",
  );
}

const TerminalWorkspace = lazy(() =>
  import("@/components/TerminalWorkspace").then((module) => ({
    default: module.TerminalWorkspace,
  })),
);

const WEB_SETTINGS_STORAGE_KEY = "vibearound.web.settings";
const LEGACY_WEB_SETTINGS_STORAGE_KEY = "vibearound.web.transcriptSettings";

const DEFAULT_WEB_SETTINGS: WebVerboseSettings = {
  show_thinking: true,
  show_tool_use: true,
  show_archived: false,
  send_with_modifier_enter: false,
};

function readStoredWebSettings(): WebVerboseSettings {
  if (typeof window === "undefined") return DEFAULT_WEB_SETTINGS;
  try {
    const raw =
      window.localStorage.getItem(WEB_SETTINGS_STORAGE_KEY) ??
      window.localStorage.getItem(LEGACY_WEB_SETTINGS_STORAGE_KEY);
    if (!raw) return DEFAULT_WEB_SETTINGS;
    const parsed = JSON.parse(raw) as Partial<WebVerboseSettings>;
    return {
      show_thinking:
        typeof parsed.show_thinking === "boolean"
          ? parsed.show_thinking
          : DEFAULT_WEB_SETTINGS.show_thinking,
      show_tool_use:
        typeof parsed.show_tool_use === "boolean"
          ? parsed.show_tool_use
          : DEFAULT_WEB_SETTINGS.show_tool_use,
      show_archived:
        typeof parsed.show_archived === "boolean"
          ? parsed.show_archived
          : DEFAULT_WEB_SETTINGS.show_archived,
      send_with_modifier_enter:
        typeof parsed.send_with_modifier_enter === "boolean"
          ? parsed.send_with_modifier_enter
          : DEFAULT_WEB_SETTINGS.send_with_modifier_enter,
    };
  } catch {
    return DEFAULT_WEB_SETTINGS;
  }
}

function App() {
  const { t } = useI18n();
  const [page, setPage] = useState<AppPage>("chat");
  const [viewMode, setViewMode] = useState<ViewMode>("tabs");
  const [theme, setTheme] = useState<Theme>(() => getResolvedTheme());
  const [webSettings, setWebSettings] = useState<WebVerboseSettings>(readStoredWebSettings);
  const [mobileSidebarOpen, setMobileSidebarOpen] = useState(false);
  const [terminalMounted, setTerminalMounted] = useState(false);

  const tmux = useTmux();
  const {
    groups,
    activeTabId,
    setActiveTabId,
    maximizedSession,
    sessionsLoading,
    addCli,
    addProfileCli,
    attachTmux,
    closeSession,
    setSessionState,
    toggleMaximize,
    clearMaximized,
  } = useSessions({
    theme,
    onTmuxAttached: tmux.refresh,
  });

  const totalSessions = groups.reduce((sum, g) => sum + g.sessions.length, 0);
  const runningSessions = groups.reduce(
    (sum, g) => sum + g.sessions.filter((s) => s.status === "running").length,
    0,
  );

  const handleActivateTab = (sessionId: string) => {
    setActiveTabId(sessionId);
    clearMaximized();
  };

  useEffect(() => {
    if (page === "terminal") setTerminalMounted(true);
  }, [page]);

  const handleWebSettingsChange = useCallback(
    (patch: Partial<WebVerboseSettings>) => {
      setWebSettings((current) => {
        const next = { ...current, ...patch };
        if (typeof window !== "undefined") {
          try {
            window.localStorage.setItem(WEB_SETTINGS_STORAGE_KEY, JSON.stringify(next));
          } catch (error) {
            console.warn("[App] failed to persist web settings:", error);
          }
        }
        return next;
      });
    },
    [],
  );

  return (
    <ThemeContext.Provider value={theme}>
      <div className="flex h-full min-h-0 overflow-hidden bg-background">
        <AppHeader
          page={page}
          onPageChange={setPage}
          mobileOpen={mobileSidebarOpen}
          onMobileOpenChange={setMobileSidebarOpen}
          viewMode={viewMode}
          onViewModeChange={setViewMode}
          theme={theme}
          onThemeToggle={() => setTheme(applyThemeToggle(theme))}
          totalSessions={totalSessions}
          runningSessions={runningSessions}
          webSettings={webSettings}
          onWebSettingsChange={handleWebSettingsChange}
        />

        <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
          {page !== "chat" && !mobileSidebarOpen && (
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              onClick={() => setMobileSidebarOpen(true)}
              className="fixed right-2 top-2 z-30 border border-border bg-background/95 text-muted-foreground shadow-sm hover:text-foreground md:hidden"
              title={t("Show navigation")}
              aria-label={t("Show navigation")}
            >
              <Menu className="h-4 w-4" />
            </Button>
          )}

          {page === "terminal" && viewMode === "tabs" && (
            <TabBar
              groups={groups}
              activeTabId={activeTabId}
              onActivate={handleActivateTab}
              onClose={closeSession}
              tmuxAvailable={tmux.available}
              tmuxSessions={tmux.sessions}
              onAddCli={addCli}
              onAddProfileCli={addProfileCli}
              onAttachTmux={attachTmux}
              onRefreshTmux={tmux.refresh}
            />
          )}

          <main className="relative min-h-0 flex-1 overflow-hidden">
            <section
              className={workspacePaneClass(page === "chat")}
              aria-hidden={page !== "chat"}
              inert={page !== "chat"}
            >
              <ChatView
                webSettings={webSettings}
                onOpenAppSidebar={() => setMobileSidebarOpen(true)}
              />
            </section>
            <section
              className={workspacePaneClass(page === "terminal")}
              aria-hidden={page !== "terminal"}
              inert={page !== "terminal"}
            >
              {terminalMounted ? (
                <Suspense fallback={<WorkspaceFallback label={t("Loading sessions")} />}>
                  <TerminalWorkspace
                    isActive={page === "terminal"}
                    groups={groups}
                    activeTabId={activeTabId}
                    maximizedSession={maximizedSession}
                    sessionsLoading={sessionsLoading}
                    viewMode={viewMode}
                    tmuxAvailable={tmux.available}
                    tmuxSessions={tmux.sessions}
                    onAddCli={addCli}
                    onAddProfileCli={addProfileCli}
                    onAttachTmux={attachTmux}
                    onRefreshTmux={tmux.refresh}
                    onToggleMaximize={toggleMaximize}
                    onCloseSession={closeSession}
                    onSessionState={setSessionState}
                  />
                </Suspense>
              ) : null}
            </section>
          </main>
        </div>
      </div>
    </ThemeContext.Provider>
  );
}

export default App;

function WorkspaceFallback({ label }: { label: string }) {
  return (
    <div className="flex h-full items-center justify-center">
      <p className="font-mono text-sm text-muted-foreground/40">{label}</p>
    </div>
  );
}
