import { CheckCircle2, Download, MessageSquare } from "lucide-react";
import { useI18n } from "@va/i18n";

import { Checkbox } from "@/components/ui/checkbox";
import { cn } from "@/lib/utils";

import type {
  DiscoveredChannelPlugin,
  PluginRegistryEntry,
} from "../types";

export function ImDecisionPanel({
  pluginRegistry,
  discoveredPlugins,
  enabledChannels,
  onToggleChannel,
}: {
  pluginRegistry: PluginRegistryEntry[];
  discoveredPlugins: DiscoveredChannelPlugin[];
  enabledChannels: Set<string>;
  onToggleChannel: (pluginId: string, enabled: boolean) => void;
}) {
  const { t } = useI18n();
  const discoveredMap = new Map(discoveredPlugins.map((plugin) => [plugin.id, plugin]));

  return (
    <div className="mx-auto flex min-h-full w-full max-w-4xl items-center py-4">
      <section className="w-full space-y-3">
        <div className="px-1">
          <div className="flex items-center gap-2 text-base font-semibold">
            <MessageSquare className="h-4 w-4 text-primary" />
            {t("Messaging apps")}
          </div>
          <p className="mt-1 text-sm text-muted-foreground">
            {t("Select the apps you want to connect.")}
          </p>
        </div>

        {pluginRegistry.length === 0 ? (
          <div className="rounded-md border border-dashed border-border px-3 py-8 text-center text-xs text-muted-foreground">
            {t("No channel plugins are available.")}
          </div>
        ) : (
          <div className="grid gap-2 lg:grid-cols-2">
            {pluginRegistry.map((entry) => {
              const selected = enabledChannels.has(entry.id);
              const discovered = discoveredMap.get(entry.id);
              const installed = Boolean(discovered);
              const installLabel =
                installed && discovered?.version
                  ? t("Installed {{version}}", { version: discovered.version })
                  : installed
                    ? t("Installed")
                    : t("Not installed");
              return (
                <button
                  key={entry.id}
                  type="button"
                  className={cn(
                    "relative flex min-h-[74px] w-full items-center gap-3 rounded-md border p-3 pr-9 text-left transition-colors",
                    selected
                      ? "border-primary/50 bg-primary/10"
                      : "border-border bg-background hover:border-primary/30",
                  )}
                  onClick={() => onToggleChannel(entry.id, !selected)}
                >
                  <Checkbox
                    checked={selected}
                    aria-hidden="true"
                    tabIndex={-1}
                    className="pointer-events-none absolute right-3 top-1/2 -translate-y-1/2"
                  />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-sm font-medium">
                      {entry.name}
                    </span>
                    <span className="block truncate text-xs text-muted-foreground">
                      {entry.description}
                    </span>
                    <span
                      className={cn(
                        "mt-1 inline-flex items-center gap-1.5 text-[11px]",
                        installed
                          ? "text-emerald-700 dark:text-emerald-300"
                          : "text-muted-foreground",
                      )}
                    >
                      {installed ? (
                        <CheckCircle2 className="h-3 w-3" />
                      ) : (
                        <Download className="h-3 w-3" />
                      )}
                      {installLabel}
                    </span>
                  </span>
                </button>
              );
            })}
          </div>
        )}
      </section>
    </div>
  );
}
