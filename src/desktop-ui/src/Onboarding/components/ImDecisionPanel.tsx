import {
  CheckCircle2,
  Download,
  MessageSquare,
} from "lucide-react";

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
    <div className="mx-auto w-full max-w-5xl space-y-5">
      <PanelSection
        icon={<MessageSquare className="h-4 w-4" />}
        title="IM tools"
        description="Each selected plugin is installed during setup and configured after setup."
      >
        {pluginRegistry.length === 0 ? (
          <div className="rounded-md border border-dashed border-border px-3 py-8 text-center text-xs text-muted-foreground">
            No channel plugins are available in the registry.
          </div>
        ) : (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(230px,1fr))] gap-2">
            {pluginRegistry.map((entry) => {
              const selected = enabledChannels.has(entry.id);
              const ready = discoveredMap.has(entry.id);
              return (
                <button
                  key={entry.id}
                  type="button"
                  className={cn(
                    "relative flex min-h-[108px] flex-col rounded-md border p-3 pr-10 text-left transition-colors",
                    selected
                      ? "border-primary/50 bg-primary/10"
                      : "border-border bg-background hover:border-primary/30",
                  )}
                  onClick={() => onToggleChannel(entry.id, !selected)}
                >
                  <span className="text-sm font-medium">{entry.name}</span>
                  <span className="mt-1 line-clamp-2 text-xs leading-5 text-muted-foreground">
                    {entry.description}
                  </span>
                  <span
                    className={cn(
                      "mt-auto inline-flex w-fit items-center gap-1.5 rounded border px-2 py-0.5 text-[11px]",
                      ready
                        ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300"
                        : "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300",
                    )}
                  >
                    {ready ? (
                      <CheckCircle2 className="h-3 w-3" />
                    ) : (
                      <Download className="h-3 w-3" />
                    )}
                    {ready ? "Installed" : "Will install"}
                  </span>
                  <Checkbox
                    checked={selected}
                    aria-hidden="true"
                    tabIndex={-1}
                    className="pointer-events-none absolute right-3 top-3"
                  />
                </button>
              );
            })}
          </div>
        )}
      </PanelSection>
    </div>
  );
}
