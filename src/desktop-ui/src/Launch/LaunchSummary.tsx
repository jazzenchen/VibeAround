import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { ChevronDown, Terminal } from "lucide-react";
import { useI18n } from "@va/i18n";

import { BrandIcon } from "@/components/brand-icon";
import { BridgeBadge } from "./LaunchBuilderPrimitives";
import type { LauncherPreferences } from "./api";
import { resolveProfileConnection } from "./connections";
import {
  agentConnectionDef,
  apiTypeProtocolDisplayLabel,
  isBridgeAgent,
} from "./launchModel";
import type { ProfileSummary } from "./types";

export type SelectorPopupId = "profile" | "terminal" | "workspace" | "session";

export interface LaunchProfileSummary {
  title: string;
  detail: string;
  bridge: boolean;
  route: string;
}

export function SelectorPopup({
  id,
  openSelector,
  onOpenChange,
  align = "start",
  widthClassName,
  widthPx,
  trigger,
  children,
}: {
  id: SelectorPopupId;
  openSelector: SelectorPopupId | null;
  onOpenChange: (id: SelectorPopupId | null) => void;
  align?: "start" | "end";
  widthClassName: string;
  widthPx: number;
  trigger: ReactNode;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const open = openSelector === id;
  const [position, setPosition] = useState<{ left: number; top: number } | null>(
    null,
  );

  useEffect(() => {
    if (!open) return;
    function closeOnOutsideClick(event: MouseEvent) {
      if (!ref.current?.contains(event.target as Node)) {
        onOpenChange(null);
      }
    }
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") onOpenChange(null);
    }
    document.addEventListener("mousedown", closeOnOutsideClick);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("mousedown", closeOnOutsideClick);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [onOpenChange, open]);

  useLayoutEffect(() => {
    if (!open) {
      setPosition(null);
      return;
    }

    function updatePosition() {
      const rect = ref.current?.getBoundingClientRect();
      if (!rect) return;

      const gutter = 8;
      const measuredWidth = contentRef.current?.getBoundingClientRect().width;
      const targetWidth =
        measuredWidth && measuredWidth > 0 ? measuredWidth : widthPx;
      const popupWidth = Math.min(targetWidth, window.innerWidth - gutter * 2);
      const rawLeft = align === "end" ? rect.right - popupWidth : rect.left;
      const maxLeft = Math.max(gutter, window.innerWidth - popupWidth - gutter);
      setPosition({
        left: Math.min(Math.max(rawLeft, gutter), maxLeft),
        top: rect.bottom + gutter,
      });
    }

    updatePosition();
    const frame = window.requestAnimationFrame(updatePosition);
    window.addEventListener("resize", updatePosition);
    window.addEventListener("scroll", updatePosition, true);
    return () => {
      window.cancelAnimationFrame(frame);
      window.removeEventListener("resize", updatePosition);
      window.removeEventListener("scroll", updatePosition, true);
    };
  }, [align, open, widthPx]);

  return (
    <div ref={ref} className="relative min-w-0">
      {trigger}
      {open && position && (
        <div
          ref={contentRef}
          className={`fixed z-50 ${widthClassName}`}
          style={{ left: position.left, top: position.top }}
        >
          {children}
        </div>
      )}
    </div>
  );
}

export function LaunchSummaryPill({
  active = false,
  disabled = false,
  label,
  title,
  detail,
  icon,
  className = "",
  chevron = false,
  onClick,
}: {
  active?: boolean;
  disabled?: boolean;
  label: string;
  title: string;
  detail?: string;
  icon?: ReactNode;
  className?: string;
  chevron?: boolean;
  onClick?: () => void;
}) {
  const content = (
    <>
      {icon && (
        <span className="flex h-5 w-5 shrink-0 items-center justify-center text-muted-foreground">
          {icon}
        </span>
      )}
      <span className="shrink-0 text-[11px] text-muted-foreground">
        {label}
      </span>
      <span className="min-w-0 truncate font-semibold text-foreground">
        {title}
      </span>
      {detail && (
        <span className="min-w-0 truncate text-muted-foreground">
          <span className="px-0.5">·</span>
          {detail}
        </span>
      )}
      {chevron && (
        <ChevronDown className="ml-auto h-3.5 w-3.5 shrink-0 text-muted-foreground" />
      )}
    </>
  );
  const interactiveClassName = onClick ? "cursor-pointer" : "";
  const baseClassName = `flex h-9 w-full min-w-0 items-center gap-1.5 overflow-hidden rounded-md border bg-transparent px-2.5 text-xs transition-colors ${
    disabled
      ? "cursor-not-allowed border-border/70 opacity-60"
      : active
        ? `${interactiveClassName} border-primary/45`
      : onClick
        ? "cursor-pointer border-border/70 hover:border-primary/35"
        : "border-border/70"
  } ${className}`;

  if (!onClick) {
    return <div className={baseClassName}>{content}</div>;
  }

  return (
    <button
      type="button"
      aria-disabled={disabled}
      tabIndex={disabled ? -1 : 0}
      className={baseClassName}
      onClick={() => {
        if (!disabled) onClick();
      }}
    >
      {content}
    </button>
  );
}

export function AgentSummaryHeader({
  agentId,
  agentLabelText,
  action,
  children,
}: {
  agentId: string;
  agentLabelText: string;
  action?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className="flex min-w-0 items-start justify-between gap-2.5">
      <span className="flex min-w-0 items-center gap-2.5">
        <span className="flex h-14 w-14 shrink-0 items-center justify-center text-primary">
          <BrandIcon
            kind="cli"
            id={agentId}
            label={agentLabelText}
            framed={false}
            className="h-12 w-12"
          />
        </span>
        <span className="min-w-0">
          <span className="block truncate text-[17px] font-semibold leading-tight">
            {agentLabelText}
          </span>
          {children}
        </span>
      </span>
      {action && <span className="shrink-0">{action}</span>}
    </div>
  );
}

function ProfileInfoRow({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <div className="grid grid-cols-[92px_minmax(0,max-content)] gap-3 border-t border-border/60 px-3 py-2">
      <div className="text-[11px] font-medium text-muted-foreground">
        {label}
      </div>
      <div className="min-w-0 max-w-[540px] text-[12px] text-foreground">
        {children}
      </div>
    </div>
  );
}

export function ProfileInfoPanel({
  agentId,
  prefs,
  profile,
  summary,
}: {
  agentId: string;
  prefs: LauncherPreferences;
  profile: ProfileSummary | null;
  summary: LaunchProfileSummary;
}) {
  const { t } = useI18n();
  const connection =
    profile && isBridgeAgent(agentId)
      ? resolveProfileConnection(
          profile,
          prefs.profileConnections,
          agentConnectionDef(agentId),
        )
      : null;
  const selectedConnection = connection?.selected ?? null;
  const launchTarget = profile?.launchTargets.find(
    (target) => target.id === agentId,
  );
  const bridgeStatus = selectedConnection
    ? selectedConnection.status === "via_bridge"
      ? t("API bridge on")
      : selectedConnection.status === "native"
        ? t("Native")
        : t("Unsupported")
    : t("Disabled");
  const modelEntries =
    selectedConnection?.status === "via_bridge"
      ? selectedConnection.models.filter((model) => model.upstreamModel)
      : [];

  return (
    <section className="overflow-hidden rounded-md border border-border bg-card shadow-sm">
      <div className="flex items-center gap-3 px-3 py-3">
        <span className="flex h-10 w-10 shrink-0 items-center justify-center rounded-md border border-border/70 bg-background text-muted-foreground">
          {profile ? (
            <BrandIcon
              kind="provider"
              id={profile.provider}
              label={profile.providerLabel}
              fallback={profile.providerIcon}
              framed={false}
              className="h-8 w-8"
            />
          ) : (
            <Terminal className="h-5 w-5" />
          )}
        </span>
        <span className="min-w-0 flex-1">
          <span className="flex min-w-0 items-center gap-2">
            <span className="truncate text-[13px] font-semibold">
              {summary.title}
            </span>
            {summary.bridge && <BridgeBadge />}
          </span>
          <span className="block truncate text-[11px] text-muted-foreground">
            {profile ? profile.providerLabel : t("Use existing CLI login")}
          </span>
        </span>
      </div>
      {profile ? (
        <>
          <ProfileInfoRow label={t("Provider")}>
            <span className="truncate">{profile.providerLabel}</span>
          </ProfileInfoRow>
          <ProfileInfoRow label={t("API kinds")}>
            <span className="truncate">
              {profile.apiTypes.map(apiTypeProtocolDisplayLabel).join(", ")}
            </span>
          </ProfileInfoRow>
          <ProfileInfoRow label={t("Route")}>
            <span className="block truncate" title={summary.route}>
              {summary.route}
            </span>
          </ProfileInfoRow>
          <ProfileInfoRow label={t("API bridge")}>
            <span className="truncate">{bridgeStatus}</span>
          </ProfileInfoRow>
          {selectedConnection && (
            <ProfileInfoRow label={t("Client API")}>
              <span className="truncate">
                {apiTypeProtocolDisplayLabel(selectedConnection.apiType)}
              </span>
            </ProfileInfoRow>
          )}
          {selectedConnection?.targetApiType && (
            <ProfileInfoRow label={t("Target API")}>
              <span className="truncate">
                {profile.providerLabel}{" "}
                {apiTypeProtocolDisplayLabel(selectedConnection.targetApiType)}
              </span>
            </ProfileInfoRow>
          )}
          {!selectedConnection && launchTarget && (
            <ProfileInfoRow label={t("Client API")}>
              <span className="truncate">
                {apiTypeProtocolDisplayLabel(launchTarget.apiType)}
              </span>
            </ProfileInfoRow>
          )}
          {modelEntries.length > 0 && (
            <ProfileInfoRow label={t("Model routes")}>
              <div className="space-y-1">
                {modelEntries.slice(0, 3).map((model, index) => (
                  <div
                    key={`${model.fakeModelId ?? ""}:${model.upstreamModel ?? ""}:${index}`}
                    className="flex min-w-0 items-center gap-1.5 font-mono text-[11px]"
                  >
                    <span className="min-w-0 truncate">
                      {model.fakeModelId || model.upstreamModel}
                    </span>
                    <span className="text-muted-foreground">-&gt;</span>
                    <span className="min-w-0 truncate">
                      {model.upstreamModel}
                    </span>
                  </div>
                ))}
                {modelEntries.length > 3 && (
                  <div className="text-[11px] text-muted-foreground">
                    {t("+{{count}} more", { count: modelEntries.length - 3 })}
                  </div>
                )}
              </div>
            </ProfileInfoRow>
          )}
        </>
      ) : (
        <>
          <ProfileInfoRow label={t("Launch mode")}>
            <span className="truncate">{t("Direct launch")}</span>
          </ProfileInfoRow>
          <ProfileInfoRow label={t("Route")}>
            <span className="truncate">{summary.route}</span>
          </ProfileInfoRow>
          <ProfileInfoRow label={t("API bridge")}>
            <span className="truncate">{t("Disabled")}</span>
          </ProfileInfoRow>
        </>
      )}
    </section>
  );
}
