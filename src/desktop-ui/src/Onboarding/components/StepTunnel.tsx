import { Globe } from "lucide-react";
import { useI18n } from "@va/i18n";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

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
  notice,
}: StepTunnelProps) {
  const { t } = useI18n();
  const orderedTunnels = [...tunnels].sort(
    (a, b) => tunnelRank(a.id) - tunnelRank(b.id),
  );

  return (
    <div className="space-y-5">
      <div>
        <h2 className="text-base font-semibold flex items-center gap-2">
          <Globe className="w-4 h-4 text-primary" />
          {t("Tunnel")}
        </h2>
        <p className="text-xs text-muted-foreground mt-1">
          {t("Expose your local server to the internet for IM webhooks and remote access. Skip if you only use it locally.")}
        </p>
        {notice}
      </div>

      <div className="flex gap-2">
        {orderedTunnels.map((tp) => (
          <Button
            key={tp.id}
            type="button"
            variant="outline"
            size="sm"
            onClick={() => onProvider(tp.id)}
            className={`flex-1 text-xs ${
              provider === tp.id
                ? "border-primary bg-primary/10 text-primary hover:bg-primary/10 hover:text-primary"
                : "text-muted-foreground"
            }`}
          >
            {tp.display_name}
            {tp.id === "cloudflare" && (
              <span className="ml-1 text-[10px] opacity-70">{t("Recommended")}</span>
            )}
          </Button>
        ))}
      </div>

      {provider === "ngrok" && (
        <div className="space-y-2">
          <label className="block">
            <span className="text-xs text-muted-foreground">{t("Auth Token")}</span>
            <Input
              type="password"
              value={ngrokToken}
              onChange={(event) => onNgrokToken(event.target.value)}
              placeholder="2ljk…"
              className="mt-1"
            />
          </label>
          <label className="block">
            <span className="text-xs text-muted-foreground">{t("Domain (optional)")}</span>
            <Input
              type="text"
              value={ngrokDomain}
              onChange={(event) => onNgrokDomain(event.target.value)}
              placeholder="myapp.ngrok-free.app"
              className="mt-1"
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
              className="mt-1"
            />
          </label>
          <label className="block">
            <span className="text-xs text-muted-foreground">{t("Hostname (optional)")}</span>
            <Input
              type="text"
              value={cfHostname}
              onChange={(event) => onCfHostname(event.target.value)}
              placeholder="vibe.yourdomain.com"
              className="mt-1"
            />
          </label>
        </div>
      )}
    </div>
  );
}

function tunnelRank(id: string): number {
  switch (id) {
    case "none":
      return 0;
    case "cloudflare":
      return 1;
    case "ngrok":
      return 2;
    case "localtunnel":
      return 3;
    default:
      return 10;
  }
}
