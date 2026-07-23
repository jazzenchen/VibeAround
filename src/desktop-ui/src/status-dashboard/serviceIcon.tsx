import { BrandIcon } from "@/components/brand-icon";
import { cn } from "@/lib/utils";
import type { Tone } from "./types";

interface ServiceIconMeta {
  src: string;
  fallback: string;
}

const CHANNEL_ICONS: Record<string, ServiceIconMeta> = {
  dingtalk: { src: "/brand/channel-dingtalk.svg", fallback: "D" },
  discord: { src: "/brand/channel-discord.svg", fallback: "D" },
  feishu: { src: "/brand/channel-feishu.svg", fallback: "F" },
  qqbot: { src: "/brand/channel-qqbot.png", fallback: "Q" },
  slack: { src: "/brand/channel-slack.svg", fallback: "S" },
  telegram: { src: "/brand/channel-telegram.svg", fallback: "T" },
  wechat: { src: "/brand/channel-wechat.svg", fallback: "W" },
  wecom: { src: "/brand/channel-wecom.svg", fallback: "W" },
};

const TUNNEL_ICONS: Record<string, ServiceIconMeta> = {
  cloudflare: { src: "/brand/tunnel-cloudflare.svg", fallback: "C" },
  localtunnel: { src: "/brand/tunnel-localtunnel.svg", fallback: "L" },
  ngrok: { src: "/brand/tunnel-ngrok.svg", fallback: "N" },
  tailscale: { src: "", fallback: "T" },
};

export function ServiceIconBadge({
  id,
  kind,
  tone,
  showStatus = true,
}: {
  id: string;
  kind: "channel" | "tunnel";
  tone: Tone;
  showStatus?: boolean;
}) {
  const meta =
    kind === "channel"
      ? CHANNEL_ICONS[id] ?? { src: "", fallback: id.slice(0, 1).toUpperCase() }
      : TUNNEL_ICONS[id] ?? { src: "", fallback: id.slice(0, 1).toUpperCase() };

  return (
    <span className="relative inline-flex h-7 w-7 shrink-0 items-center justify-center">
      <span className="flex h-full w-full items-center justify-center">
        {meta.src ? (
          <img
            src={meta.src}
            alt=""
            draggable={false}
            className="h-[82%] w-[82%] object-contain"
          />
        ) : (
          <span className="text-[11px] font-semibold text-primary">
            {meta.fallback}
          </span>
        )}
      </span>
      {showStatus && (
        <span
          className={cn(
            "absolute -right-0.5 -top-0.5 h-2.5 w-2.5 rounded-full border-2 border-card",
            toneDot(tone),
          )}
        />
      )}
    </span>
  );
}

export function AgentIconBadge({
  cliKind,
  label,
  tone,
  showStatus = true,
}: {
  cliKind: string | null;
  label: string;
  tone: Tone;
  showStatus?: boolean;
}) {
  const id = cliKind?.toLowerCase() ?? "agent";

  return (
    <span className="relative inline-flex h-7 w-7 shrink-0">
      <BrandIcon
        kind="cli"
        id={id}
        label={label}
        framed={false}
        className="h-7 w-7"
      />
      {showStatus && (
        <span
          className={cn(
            "absolute -right-0.5 -top-0.5 h-2.5 w-2.5 rounded-full border-2 border-card",
            toneDot(tone),
          )}
        />
      )}
    </span>
  );
}

function toneDot(tone: Tone) {
  switch (tone) {
    case "good":
      return "bg-emerald-500";
    case "busy":
      return "bg-primary";
    case "warning":
      return "bg-amber-500";
    case "danger":
      return "bg-destructive";
    case "muted":
      return "bg-muted-foreground/40";
  }
}
