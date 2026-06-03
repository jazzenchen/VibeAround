import { ChevronDown, Globe } from "lucide-react";
import { useState } from "react";

import { Button } from "@/components/ui/button";
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
  const [showMore, setShowMore] = useState(false);
  const cloudflare = tunnels.find((tunnel) => tunnel.id === "cloudflare");
  const none = tunnels.find((tunnel) => tunnel.id === "none");
  const moreTunnels = tunnels
    .filter((tunnel) => tunnel.id !== "cloudflare" && tunnel.id !== "none")
    .sort((a, b) => tunnelRank(a.id) - tunnelRank(b.id));

  return (
    <div className="mx-auto flex min-h-full w-full max-w-3xl items-center py-8">
      <PanelSection
        icon={<Globe className="h-4 w-4" />}
        title="Remote access"
        description="Cloudflare is recommended when you need a stable public URL."
      >
        <div className="grid gap-2 sm:grid-cols-2">
          {cloudflare && (
            <TunnelCard
              tunnel={cloudflare}
              selected={provider === cloudflare.id}
              recommended
              onSelect={() => onProvider(cloudflare.id)}
            />
          )}
          {none && (
            <TunnelCard
              tunnel={none}
              selected={provider === none.id}
              onSelect={() => onProvider(none.id)}
            />
          )}
        </div>

        {moreTunnels.length > 0 && (
          <div className="mt-3">
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="px-0 text-xs text-muted-foreground hover:bg-transparent"
              onClick={() => setShowMore((value) => !value)}
            >
              <ChevronDown
                className={cn(
                  "h-3.5 w-3.5 transition-transform",
                  showMore && "rotate-180",
                )}
              />
              {showMore ? "Hide other options" : "Other options"}
            </Button>
            {showMore && (
              <div className="mt-2 grid gap-2 sm:grid-cols-2 animate-in fade-in slide-in-from-top-1 duration-200">
                {moreTunnels.map((tunnel) => (
                  <TunnelCard
                    key={tunnel.id}
                    tunnel={tunnel}
                    selected={provider === tunnel.id}
                    onSelect={() => onProvider(tunnel.id)}
                  />
                ))}
              </div>
            )}
          </div>
        )}
      </PanelSection>
    </div>
  );
}

function TunnelCard({
  tunnel,
  selected,
  recommended,
  onSelect,
}: {
  tunnel: TunnelSummary;
  selected: boolean;
  recommended?: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      className={cn(
        "min-h-[112px] rounded-md border p-4 text-left transition-colors",
        selected
          ? "border-primary/50 bg-primary/10"
          : "border-border bg-background hover:border-primary/30",
      )}
      onClick={onSelect}
    >
      <span className="flex items-center justify-between gap-2">
        <span className="text-sm font-medium">{tunnel.display_name}</span>
        {recommended && (
          <span className="rounded-full border border-primary/25 bg-primary/10 px-2 py-0.5 text-[10px] text-primary">
            Recommended
          </span>
        )}
      </span>
      <span className="mt-2 block text-xs leading-5 text-muted-foreground">
        {tunnelDescription(tunnel.id)}
      </span>
    </button>
  );
}
