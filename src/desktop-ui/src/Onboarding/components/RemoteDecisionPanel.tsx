import { Globe } from "lucide-react";

import { cn } from "@/lib/utils";

import { PanelSection } from "./PanelSection";
import {
  tunnelDescription,
  tunnelRank,
} from "./startkitPresentation";
import type { TunnelSummary } from "../types";
import type { TunnelProvider } from "../constants";

export function RemoteDecisionPanel({
  tunnels,
  provider,
  onProvider,
}: {
  tunnels: TunnelSummary[];
  provider: TunnelProvider;
  onProvider: (value: TunnelProvider) => void;
}) {
  const orderedTunnels = [...tunnels].sort(
    (a, b) => tunnelRank(a.id) - tunnelRank(b.id),
  );

  return (
    <div className="mx-auto w-full max-w-5xl space-y-5">
      <PanelSection
        icon={<Globe className="h-4 w-4" />}
        title="Remote access"
        description="Cloudflare installs the local binary now; token and hostname are entered after setup."
      >
        <div className="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-2">
          {orderedTunnels.map((tunnel) => {
            const selected = provider === tunnel.id;
            return (
              <button
                key={tunnel.id}
                type="button"
                className={cn(
                  "min-h-[96px] rounded-md border p-3 text-left transition-colors",
                  selected
                    ? "border-primary/50 bg-primary/10"
                    : "border-border bg-background hover:border-primary/30",
                )}
                onClick={() => onProvider(tunnel.id)}
              >
                <span className="flex items-center justify-between gap-2">
                  <span className="text-sm font-medium">
                    {tunnel.display_name}
                  </span>
                  {tunnel.id === "cloudflare" && (
                    <span className="rounded border border-primary/25 bg-primary/10 px-1.5 py-0.5 text-[10px] text-primary">
                      Recommended
                    </span>
                  )}
                </span>
                <span className="mt-2 block text-xs leading-5 text-muted-foreground">
                  {tunnelDescription(tunnel.id)}
                </span>
              </button>
            );
          })}
        </div>
      </PanelSection>
    </div>
  );
}
