import { BrandIcon } from "@/components/brand-icon";
import { cn } from "@/lib/utils";

interface SessionHostLogoProps {
  agentId: string;
  agentLabel?: string;
  profileId?: string | null;
  profileLabel?: string | null;
  providerId?: string | null;
  providerLabel?: string | null;
  archived?: boolean;
  size?: "sm" | "md";
  className?: string;
}

const DIRECT_PROFILE_ID = "direct";

export function hasSessionHostProfileLogo(
  profileId?: string | null,
  providerId?: string | null,
) {
  return Boolean(providerId || (profileId && profileId !== DIRECT_PROFILE_ID));
}

export function SessionHostLogo({
  agentId,
  agentLabel,
  profileId,
  profileLabel,
  providerId,
  providerLabel,
  archived = false,
  size = "sm",
  className,
}: SessionHostLogoProps) {
  const hasProfile = hasSessionHostProfileLogo(profileId, providerId);
  const iconSize = size === "md" ? "h-8 w-8" : "h-6 w-6";
  const frameSize = size === "md" ? "h-10 w-12" : "h-7 w-9";
  const profileDisplayLabel = providerLabel ?? profileLabel;
  const profileBrandId = providerId ?? profileId ?? "";
  const title = [agentLabel, profileDisplayLabel].filter(Boolean).join(" / ");

  return (
    <span
      className={cn("relative inline-flex shrink-0", frameSize, archived && "opacity-50", className)}
      title={title || undefined}
      aria-label={title || undefined}
    >
      {hasProfile && (
        <BrandIcon
          kind="provider"
          id={profileBrandId}
          label={profileDisplayLabel ?? undefined}
          fallback={(profileDisplayLabel ?? profileBrandId).slice(0, 1).toUpperCase()}
          className={cn(
            "absolute right-0 top-1/2 -translate-y-1/2 rounded-full border border-background bg-background shadow-sm",
            iconSize,
          )}
        />
      )}
      <BrandIcon
        kind="cli"
        id={agentId}
        label={agentLabel}
        className={cn(
          "absolute left-0 z-10 rounded-full border border-background bg-background shadow-sm",
          "top-1/2 -translate-y-1/2",
          iconSize,
        )}
      />
    </span>
  );
}
