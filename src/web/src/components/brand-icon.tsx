import { Bot, Sparkles } from "lucide-react";

import { cn } from "@/lib/utils";

const CLI_LOGOS: Record<string, string> = {
  claude: "brand/cli-claude.svg",
  codex: "brand/cli-openai.svg",
  pi: "brand/cli-pi.svg",
  gemini: "brand/cli-gemini.svg",
  opencode: "brand/cli-opencode.svg",
  cursor: "brand/cli-cursor.svg",
  kiro: "brand/cli-kiro.svg",
  "qwen-code": "brand/cli-qwen.svg",
};

const PROVIDER_LOGOS: Record<string, string> = {
  azure: "brand/provider-azure.svg",
  dashscope: "brand/provider-dashscope.svg",
  deepseek: "brand/provider-deepseek-color.svg",
  gemini: "brand/provider-gemini-color.svg",
  kimi: "brand/provider-moonshot.webp",
  mimo: "brand/provider-mimo.svg",
  minimax: "brand/provider-minimax-color.svg",
  moonshot: "brand/provider-moonshot.webp",
  nvidia: "brand/provider-nvidia.svg",
  openrouter: "brand/provider-openrouter-color.svg",
  volcengine: "brand/provider-volcengine.svg",
  xai: "brand/provider-xai.svg",
  zai: "brand/provider-zai-color.svg",
};

interface BrandIconProps {
  kind: "cli" | "provider";
  id: string;
  label?: string;
  fallback?: string | null;
  framed?: boolean;
  className?: string;
}

export function BrandIcon({
  kind,
  id,
  label,
  fallback,
  framed = false,
  className,
}: BrandIconProps) {
  const src = kind === "cli" ? CLI_LOGOS[id] : PROVIDER_LOGOS[id];
  const assetSrc = src ? `${import.meta.env.BASE_URL}${src}` : undefined;
  const fallbackClass = cn(
    "inline-flex shrink-0 items-center justify-center overflow-hidden",
    framed && "rounded-md border border-border/70 bg-background",
    className,
  );

  if (src) {
    return (
      <span className={fallbackClass}>
        <img
          src={assetSrc}
          alt={label ? `${label} logo` : ""}
          draggable={false}
          className="h-[78%] w-[78%] object-contain"
        />
      </span>
    );
  }

  if (fallback) {
    return <span className={cn(fallbackClass, "text-[0.8em]")}>{fallback}</span>;
  }

  const FallbackIcon = kind === "cli" ? Bot : Sparkles;
  return (
    <span className={cn(fallbackClass, "text-primary")}>
      <FallbackIcon className="h-[60%] w-[60%]" />
    </span>
  );
}
