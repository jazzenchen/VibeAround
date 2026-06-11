import {
  Grid3x3,
  LayoutGrid,
  MessageSquare,
  Moon,
  Rows3,
  Sun,
  Terminal,
} from "lucide-react";
import type { WebVerboseSettings } from "@va/client";
import { useI18n } from "@va/i18n";

import { Button } from "@/components/ui/button";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import type { AppPage, ChatRuntimeStatus } from "@/lib/dashboard-types";
import type { Theme } from "@/lib/theme";
import type { ViewMode } from "@/lib/terminal-types";
import { cn } from "@/lib/utils";
import { ChatSettingsMenu } from "./chat/ChatSettingsMenu";
import { LanguageMenu } from "./LanguageMenu";

interface AppHeaderProps {
  page: AppPage;
  onPageChange: (page: AppPage) => void;
  mobileOpen?: boolean;
  onMobileOpenChange?: (open: boolean) => void;
  viewMode: ViewMode;
  onViewModeChange: (mode: ViewMode) => void;
  theme: Theme;
  onThemeToggle: () => void;
  totalSessions: number;
  runningSessions: number;
  chatStatus: ChatRuntimeStatus;
  webSettings: WebVerboseSettings;
  onWebSettingsChange: (patch: Partial<WebVerboseSettings>) => void;
}

export function AppHeader({
  page,
  onPageChange,
  mobileOpen = false,
  onMobileOpenChange,
  viewMode,
  onViewModeChange,
  theme,
  onThemeToggle,
  totalSessions,
  runningSessions,
  chatStatus,
  webSettings,
  onWebSettingsChange,
}: AppHeaderProps) {
  const { t } = useI18n();
  const chatStatusMeta = {
    connecting: {
      label: t("Connecting to local agent"),
    },
    ready: {
      label: t("Local agent ready"),
    },
    working: {
      label: t("Agent working"),
    },
    attention: {
      label: t("Agent needs input"),
    },
  } satisfies Record<ChatRuntimeStatus, { label: string }>;
  const chatMeta = chatStatusMeta[chatStatus];
  const terminalTitle =
    totalSessions > 0
      ? t("{{running}}/{{total}} CLI", {
          running: runningSessions,
          total: totalSessions,
        })
      : t("CLI");
  const selectPage = (nextPage: AppPage) => {
    onPageChange(nextPage);
    onMobileOpenChange?.(false);
  };

  return (
    <>
      <aside className="hidden h-full w-14 shrink-0 flex-col items-center border-r border-border bg-background/95 px-1.5 py-3 md:flex">
        <VibeAroundLogo className="h-9 w-9" />

        <nav className="mt-5 flex flex-1 flex-col items-center gap-2">
          <Button
            type="button"
            variant={page === "chat" ? "secondary" : "ghost"}
            size="icon-sm"
            onClick={() => selectPage("chat")}
            className={cn(
              "relative h-9 w-9 text-muted-foreground hover:text-foreground",
              page === "chat" && "bg-primary/15 text-primary hover:text-primary",
            )}
            title={`${t("Agent")} · ${chatMeta.label}`}
            aria-label={t("Agent")}
          >
            <MessageSquare className="h-4 w-4" />
          </Button>
          <Button
            type="button"
            variant={page === "terminal" ? "secondary" : "ghost"}
            size="icon-sm"
            onClick={() => selectPage("terminal")}
            className={cn(
              "relative h-9 w-9 text-muted-foreground hover:text-foreground",
              page === "terminal" && "bg-primary/15 text-primary hover:text-primary",
            )}
            title={terminalTitle}
            aria-label={t("CLI")}
          >
            <Terminal className="h-4 w-4" />
            {totalSessions > 0 && (
              <span className="absolute -right-0.5 -top-0.5 flex h-4 min-w-4 items-center justify-center rounded-full bg-muted px-1 font-mono text-[9px] leading-none text-muted-foreground">
                {totalSessions}
              </span>
            )}
          </Button>

          {page === "terminal" && (
            <ToggleGroup
              type="single"
              value={viewMode}
              onValueChange={(v) => v && onViewModeChange(v as ViewMode)}
              className="mt-3 flex flex-col items-center gap-1 rounded-md bg-muted/70 p-1"
            >
              <ToggleGroupItem
                value="tabs"
                aria-label={t("Tab view")}
                title={t("Tab view")}
                className="h-7 w-7 rounded p-0 data-[state=on]:bg-primary/15 data-[state=on]:text-primary text-muted-foreground/50 hover:text-foreground"
              >
                <Rows3 className="h-3.5 w-3.5" />
              </ToggleGroupItem>
              <ToggleGroupItem
                value="grid"
                aria-label={t("Grid view")}
                title={t("Grid view")}
                className="h-7 w-7 rounded p-0 data-[state=on]:bg-primary/15 data-[state=on]:text-primary text-muted-foreground/50 hover:text-foreground"
              >
                <LayoutGrid className="h-3.5 w-3.5" />
              </ToggleGroupItem>
              <ToggleGroupItem
                value="nine"
                aria-label={t("Nine-grid view")}
                title={t("Nine-grid view")}
                className="h-7 w-7 rounded p-0 data-[state=on]:bg-primary/15 data-[state=on]:text-primary text-muted-foreground/50 hover:text-foreground"
              >
                <Grid3x3 className="h-3.5 w-3.5" />
              </ToggleGroupItem>
            </ToggleGroup>
          )}
        </nav>

        <div className="flex flex-col items-center gap-2">
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            onClick={onThemeToggle}
            className="h-8 w-8 text-muted-foreground hover:text-foreground"
            aria-label={theme === "dark" ? t("Switch to light theme") : t("Switch to dark theme")}
            title={theme === "dark" ? t("Switch to light theme") : t("Switch to dark theme")}
          >
            {theme === "dark" ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
          </Button>
          <LanguageMenu />
          <ChatSettingsMenu settings={webSettings} onChange={onWebSettingsChange} />
        </div>
      </aside>

      {mobileOpen && (
        <div className="fixed inset-0 z-40 md:hidden">
          <button
            type="button"
            className="absolute inset-0 bg-background/70 backdrop-blur-sm"
            aria-label={t("Close navigation")}
            onClick={() => onMobileOpenChange?.(false)}
          />
          <aside className="absolute inset-y-0 right-0 z-10 flex w-[min(18rem,86vw)] flex-col border-l border-border bg-background shadow-xl">
            <div className="border-b border-border/70 p-4">
              <div className="flex items-center gap-3">
                <VibeAroundLogo className="h-10 w-10" />
                <div className="min-w-0">
                  <div className="truncate text-sm font-semibold text-foreground">
                    VibeAround
                  </div>
                </div>
              </div>
            </div>

            <nav className="min-h-0 flex-1 space-y-2 overflow-y-auto p-3">
              <button
                type="button"
                onClick={() => selectPage("chat")}
                className={cn(
                  "flex w-full items-center gap-3 rounded-md border px-3 py-2.5 text-left transition-colors",
                  page === "chat"
                    ? "border-primary/40 bg-primary/10 text-foreground"
                    : "border-border bg-muted/20 text-muted-foreground hover:bg-muted/50 hover:text-foreground",
                )}
              >
                <span className="relative flex h-9 w-9 shrink-0 items-center justify-center rounded-md bg-background text-primary">
                  <MessageSquare className="h-4 w-4" />
                </span>
                <span className="min-w-0">
                  <span className="block truncate text-sm font-medium">{t("Chat")}</span>
                  <span className="block truncate font-mono text-[11px] text-muted-foreground/70">
                    {chatMeta.label}
                  </span>
                </span>
              </button>

              <button
                type="button"
                onClick={() => selectPage("terminal")}
                className={cn(
                  "flex w-full items-center gap-3 rounded-md border px-3 py-2.5 text-left transition-colors",
                  page === "terminal"
                    ? "border-primary/40 bg-primary/10 text-foreground"
                    : "border-border bg-muted/20 text-muted-foreground hover:bg-muted/50 hover:text-foreground",
                )}
              >
                <span className="relative flex h-9 w-9 shrink-0 items-center justify-center rounded-md bg-background">
                  <Terminal className="h-4 w-4" />
                  {totalSessions > 0 && (
                    <span className="absolute -right-1 -top-1 flex h-4 min-w-4 items-center justify-center rounded-full bg-muted px-1 font-mono text-[9px] leading-none text-muted-foreground">
                      {totalSessions}
                    </span>
                  )}
                </span>
                <span className="min-w-0">
                  <span className="block truncate text-sm font-medium">{t("CLI")}</span>
                  <span className="block truncate font-mono text-[11px] text-muted-foreground/70">
                    {terminalTitle}
                  </span>
                </span>
              </button>

              {page === "terminal" && (
                <div className="rounded-md border border-border bg-muted/20 p-2">
                  <div className="mb-2 px-1 font-mono text-[10px] uppercase tracking-[0.14em] text-muted-foreground/60">
                    {t("View")}
                  </div>
                  <ToggleGroup
                    type="single"
                    value={viewMode}
                    onValueChange={(v) => v && onViewModeChange(v as ViewMode)}
                    className="grid grid-cols-3 gap-1"
                  >
                    <ToggleGroupItem
                      value="tabs"
                      aria-label={t("Tab view")}
                      title={t("Tab view")}
                      className="h-8 rounded-md data-[state=on]:bg-primary/15 data-[state=on]:text-primary text-muted-foreground/70 hover:text-foreground"
                    >
                      <Rows3 className="mr-1.5 h-3.5 w-3.5" />
                      <span className="text-xs">{t("Tabs")}</span>
                    </ToggleGroupItem>
                    <ToggleGroupItem
                      value="grid"
                      aria-label={t("Grid view")}
                      title={t("Grid view")}
                      className="h-8 rounded-md data-[state=on]:bg-primary/15 data-[state=on]:text-primary text-muted-foreground/70 hover:text-foreground"
                    >
                      <LayoutGrid className="mr-1.5 h-3.5 w-3.5" />
                      <span className="text-xs">{t("Grid")}</span>
                    </ToggleGroupItem>
                    <ToggleGroupItem
                      value="nine"
                      aria-label={t("Nine-grid view")}
                      title={t("Nine-grid view")}
                      className="h-8 rounded-md data-[state=on]:bg-primary/15 data-[state=on]:text-primary text-muted-foreground/70 hover:text-foreground"
                    >
                      <Grid3x3 className="mr-1.5 h-3.5 w-3.5" />
                      <span className="text-xs">{t("Nine grid")}</span>
                    </ToggleGroupItem>
                  </ToggleGroup>
                </div>
              )}
            </nav>

            <div className="flex items-center justify-between gap-2 border-t border-border/70 p-3">
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={onThemeToggle}
                className="justify-start gap-2 text-muted-foreground hover:text-foreground"
                aria-label={theme === "dark" ? t("Switch to light theme") : t("Switch to dark theme")}
                title={theme === "dark" ? t("Switch to light theme") : t("Switch to dark theme")}
              >
                {theme === "dark" ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
                <span>{theme === "dark" ? t("Light") : t("Dark")}</span>
              </Button>
              <LanguageMenu />
              <ChatSettingsMenu settings={webSettings} onChange={onWebSettingsChange} />
            </div>
          </aside>
        </div>
      )}
    </>
  );
}

function VibeAroundLogo({ className }: { className?: string }) {
  return (
    <img
      src="/brand/vibearound-mark.svg"
      alt=""
      aria-hidden="true"
      draggable={false}
      className={cn("shrink-0 rounded-md", className)}
    />
  );
}
