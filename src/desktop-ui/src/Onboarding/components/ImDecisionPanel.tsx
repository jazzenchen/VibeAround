import { CheckCircle2, Download, MessageSquare } from "lucide-react";

import { Checkbox } from "@/components/ui/checkbox";
import { cn } from "@/lib/utils";

import { PanelSection } from "./PanelSection";
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
  const discoveredMap = new Map(discoveredPlugins.map((plugin) => [plugin.id, plugin]));

  return (
    <div className="mx-auto flex min-h-full w-full max-w-3xl items-center py-8">
      <PanelSection
        icon={<MessageSquare className="h-4 w-4" />}
        title="Use IM access"
        description="Select only the apps you actually use. Login happens later."
      >
        {pluginRegistry.length === 0 ? (
          <div className="rounded-md border border-dashed border-border px-3 py-8 text-center text-xs text-muted-foreground">
            No channel plugins are available.
          </div>
        ) : (
          <div className="space-y-2">
            {pluginRegistry.map((entry) => {
              const selected = enabledChannels.has(entry.id);
              const installed = discoveredMap.has(entry.id);
              return (
                <button
                  key={entry.id}
                  type="button"
                  className={cn(
                    "flex w-full items-center gap-3 rounded-md border p-3 text-left transition-colors",
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
                    className="pointer-events-none"
                  />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-sm font-medium">
                      {entry.name}
                    </span>
                    <span className="block truncate text-xs text-muted-foreground">
                      {entry.description}
                    </span>
                  </span>
                  <span
                    className={cn(
                      "hidden shrink-0 items-center gap-1.5 rounded-full border px-2 py-1 text-[11px] sm:inline-flex",
                      installed
                        ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300"
                        : "border-border bg-muted text-muted-foreground",
                    )}
                  >
                    {installed ? (
                      <CheckCircle2 className="h-3 w-3" />
                    ) : (
                      <Download className="h-3 w-3" />
                    )}
                    {installed ? "Installed" : "Not installed"}
                  </span>
                </button>
              );
            })}
          </div>
        )}
      </PanelSection>
    </div>
  );
}
