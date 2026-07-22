import {
  Check,
  Cloud,
  Globe2,
  Laptop,
  Link2,
  RadioTower,
} from "lucide-react";
import { useI18n } from "@va/i18n";

import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

import type { StepTunnelProps } from "../types";

export function StepTunnel({
  tunnels,
  provider,
  onProvider,
  ngrokToken,
  onNgrokToken,
  ngrokDomain,
  onNgrokDomain,
  cfToken,
  onCfToken,
  cfHostname,
  onCfHostname,
  showProviderSelect = false,
  compact = false,
  notice,
}: StepTunnelProps) {
  const { t } = useI18n();
  const selectedTunnel = tunnels.find((tunnel) => tunnel.id === provider);
  const inputClassName = compact ? "mt-1 h-8 text-xs" : "mt-1";

  if (!selectedTunnel && !showProviderSelect) return null;

  return (
    <div className="space-y-4">
      {showProviderSelect ? (
        <>
          <div>
            <h2 className="flex items-center gap-2 text-base font-semibold">
              <Globe2 className="h-4 w-4 text-primary" />
              {t("Tunnel")}
            </h2>
            <p className="mt-1 text-xs text-muted-foreground">
              {t("Choose how VibeAround exposes this computer for remote access.")}
            </p>
            {notice}
          </div>
          <div className="grid gap-2 [grid-template-columns:repeat(auto-fit,minmax(140px,1fr))]">
            {orderedTunnels(tunnels).map((tunnel) => {
              const selected = tunnel.id === provider;
              const Icon = tunnelIcon(tunnel.id);
              return (
                <button
                  key={tunnel.id}
                  type="button"
                  aria-pressed={selected}
                  title={tunnelSettingsDescription(tunnel.id, t)}
                  className={cn(
                    "flex h-10 min-w-0 items-center gap-2 rounded-md border px-2.5 text-left transition-colors",
                    selected
                      ? "border-primary/45 bg-primary/10 text-primary"
                      : "border-border bg-background text-foreground hover:border-primary/30 hover:bg-muted/20",
                  )}
                  onClick={() => onProvider(tunnel.id)}
                >
                  <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md border border-border/70 bg-muted/30">
                    <Icon className="h-3.5 w-3.5" />
                  </span>
                  <span className="min-w-0 flex-1 truncate text-xs font-medium">
                    {tunnel.display_name}
                  </span>
                  {selected && (
                    <span className="flex h-4 w-4 shrink-0 items-center justify-center rounded-[4px] bg-primary text-primary-foreground">
                      <Check className="h-3 w-3" />
                    </span>
                  )}
                </button>
              );
            })}
          </div>
          {provider === "cloudflare" && (
            <div className="text-[11px] text-primary">
              {t("Cloudflare Tunnel is recommended for stable remote access.")}
            </div>
          )}
        </>
      ) : (
        selectedTunnel && (
          <div className="flex gap-2">
            <div className="flex min-h-8 flex-1 items-center justify-center rounded-md border border-primary bg-primary/10 px-3 py-1.5 text-xs font-medium text-primary">
              {selectedTunnel.display_name}
              {selectedTunnel.id === "cloudflare" && (
                <span className="ml-1 text-[10px] opacity-70">
                  {t("Recommended")}
                </span>
              )}
            </div>
          </div>
        )
      )}

      {provider === "none" && (
        <div className="rounded-md border border-border bg-muted/20 px-4 py-4 text-xs text-muted-foreground">
          {t("Remote access is disabled. VibeAround stays available on this computer only.")}
        </div>
      )}

      {provider === "localtunnel" && (
        <div className="rounded-md border border-border bg-muted/20 px-4 py-4 text-xs text-muted-foreground">
          {t("LocalTunnel does not require credentials. Save and restart services to apply this provider.")}
        </div>
      )}

      {provider === "tailscale" && (
        <div className="rounded-md border border-border bg-muted/20 px-4 py-4 text-xs text-muted-foreground">
          {t("Tailscale must be installed and signed in. Funnel publishes the Web Hub to the internet; when approval is required, use the Enable Funnel button in Remote Access.")}
        </div>
      )}

      {provider === "ngrok" && (
        <div className="space-y-2">
          <label className="block">
            <span className="text-xs text-muted-foreground">{t("Auth Token")}</span>
            <Input
              type="password"
              value={ngrokToken}
              onChange={(event) => onNgrokToken(event.target.value)}
              placeholder="2ljk…"
              className={inputClassName}
            />
          </label>
          <label className="block">
            <span className="text-xs text-muted-foreground">{t("Domain (optional)")}</span>
            <Input
              type="text"
              value={ngrokDomain}
              onChange={(event) => onNgrokDomain(event.target.value)}
              placeholder="myapp.ngrok-free.app"
              className={inputClassName}
            />
          </label>
        </div>
      )}

      {provider === "cloudflare" && (
        <div className="space-y-2">
          <label className="block">
            <span className="text-xs text-muted-foreground">{t("Tunnel Token")}</span>
            <Input
              type="password"
              value={cfToken}
              onChange={(event) => onCfToken(event.target.value)}
              placeholder="eyJh…"
              className={inputClassName}
            />
          </label>
          <label className="block">
            <span className="text-xs text-muted-foreground">{t("Hostname (optional)")}</span>
            <Input
              type="text"
              value={cfHostname}
              onChange={(event) => onCfHostname(event.target.value)}
              placeholder="vibe.yourdomain.com"
              className={inputClassName}
            />
          </label>
        </div>
      )}
    </div>
  );
}

function tunnelIcon(id: string) {
  switch (id) {
    case "none":
      return Laptop;
    case "cloudflare":
      return Cloud;
    case "localtunnel":
      return Link2;
    case "ngrok":
      return RadioTower;
    case "tailscale":
      return Globe2;
    default:
      return Globe2;
  }
}

function orderedTunnels(tunnels: StepTunnelProps["tunnels"]) {
  const rank = new Map([
    ["none", 0],
    ["cloudflare", 1],
    ["tailscale", 2],
    ["localtunnel", 3],
    ["ngrok", 4],
  ]);
  return [...tunnels].sort(
    (a, b) => (rank.get(a.id) ?? 99) - (rank.get(b.id) ?? 99),
  );
}

function tunnelSettingsDescription(
  id: string,
  t: (key: string) => string,
) {
  switch (id) {
    case "none":
      return t("Local access only; no public tunnel is started.");
    case "cloudflare":
      return t("Stable remote access through a Cloudflare Tunnel token.");
    case "localtunnel":
      return t("Quick public URL through localtunnel, with no API key.");
    case "ngrok":
      return t("Remote access through an Ngrok account token.");
    case "tailscale":
      return t("Public HTTPS access through Tailscale Funnel, with no separate tunnel token.");
    default:
      return t("Remote access provider.");
  }
}
