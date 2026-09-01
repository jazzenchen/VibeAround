import { useCallback, useState } from "react";
import type { WebVerboseSettings } from "@va/client";

import { AppHeader } from "@/components/AppHeader";
import { ChatView } from "@/components/chat";
import { ThemeContext, getResolvedTheme, toggleTheme as applyThemeToggle, type Theme } from "@/lib/theme";

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
  const [theme, setTheme] = useState<Theme>(() => getResolvedTheme());
  const [webSettings, setWebSettings] = useState<WebVerboseSettings>(readStoredWebSettings);
  const [mobileSidebarOpen, setMobileSidebarOpen] = useState(false);

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
          mobileOpen={mobileSidebarOpen}
          onMobileOpenChange={setMobileSidebarOpen}
          theme={theme}
          onThemeToggle={() => setTheme(applyThemeToggle(theme))}
          webSettings={webSettings}
          onWebSettingsChange={handleWebSettingsChange}
        />

        <main className="relative min-h-0 flex-1 overflow-hidden">
          <ChatView
            webSettings={webSettings}
            onOpenAppSidebar={() => setMobileSidebarOpen(true)}
          />
        </main>
      </div>
    </ThemeContext.Provider>
  );
}

export default App;
