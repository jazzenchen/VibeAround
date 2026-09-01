import { Moon, Sun } from "lucide-react";
import type { WebVerboseSettings } from "@va/client";
import { useI18n } from "@va/i18n";

import { Button } from "@/components/ui/button";
import type { Theme } from "@/lib/theme";
import { cn } from "@/lib/utils";
import { ChatSettingsMenu } from "./chat/ChatSettingsMenu";
import { LanguageMenu } from "./LanguageMenu";

interface AppHeaderProps {
  mobileOpen?: boolean;
  onMobileOpenChange?: (open: boolean) => void;
  theme: Theme;
  onThemeToggle: () => void;
  webSettings: WebVerboseSettings;
  onWebSettingsChange: (patch: Partial<WebVerboseSettings>) => void;
}

export function AppHeader({
  mobileOpen = false,
  onMobileOpenChange,
  theme,
  onThemeToggle,
  webSettings,
  onWebSettingsChange,
}: AppHeaderProps) {
  const { t } = useI18n();

  return (
    <>
      <aside className="hidden h-full w-14 shrink-0 flex-col items-center border-r border-border bg-background/95 px-1.5 py-3 md:flex">
        <VibeAroundLogo className="h-9 w-9" />

        <div className="mt-auto flex flex-col items-center gap-2">
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

            <div className="min-h-0 flex-1 overflow-y-auto p-3" />

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
      src={`${import.meta.env.BASE_URL}brand/vibearound-mark.svg`}
      alt=""
      aria-hidden="true"
      draggable={false}
      className={cn("shrink-0", className)}
    />
  );
}
