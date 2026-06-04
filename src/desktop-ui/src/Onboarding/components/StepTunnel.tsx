import { useI18n } from "@va/i18n";

import { Input } from "@/components/ui/input";

import type { StepTunnelProps } from "../types";

export function StepTunnel({
  tunnels,
  provider,
  ngrokToken,
  onNgrokToken,
  ngrokDomain,
  onNgrokDomain,
  cfToken,
  onCfToken,
  cfHostname,
  onCfHostname,
}: StepTunnelProps) {
  const { t } = useI18n();
  const selectedTunnel = tunnels.find((tunnel) => tunnel.id === provider);

  if (!selectedTunnel || provider === "none") return null;

  return (
    <div className="space-y-4">
      <div className="flex gap-2">
        <div className="flex min-h-8 flex-1 items-center justify-center rounded-md border border-primary bg-primary/10 px-3 py-1.5 text-xs font-medium text-primary">
          {selectedTunnel.display_name}
          {selectedTunnel.id === "cloudflare" && (
            <span className="ml-1 text-[10px] opacity-70">{t("Recommended")}</span>
          )}
        </div>
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
