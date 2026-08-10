"use client";

import {
  Menu,
  PanelLeftClose,
  PanelLeftOpen,
  Wifi,
  WifiOff,
} from "lucide-react";
import { useI18n } from "@va/i18n";
import { Button } from "@/components/ui/button";
import { shortSessionId } from "./chatSessionDisplay";
import { SessionHostLogo } from "./SessionHostLogo";

interface ChatHeaderProps {
  selectedAgent: string;
  agentLabel: string;
  profileId?: string | null;
  profileLabel?: string | null;
  providerId?: string | null;
  providerLabel?: string | null;
  routeLabel: string;
  headerSessionLabel: string | null;
  workspacePath?: string;
  sessionId?: string;
  connected: boolean;
  showSessionSidebar: boolean;
  onShowMobileSessions: () => void;
  onToggleSessionSidebar: () => void;
  onOpenAppSidebar?: () => void;
}

export function ChatHeader({
  selectedAgent,
  agentLabel,
  profileId,
  profileLabel,
  providerId,
  providerLabel,
  routeLabel,
  headerSessionLabel,
  workspacePath,
  sessionId,
  connected,
  showSessionSidebar,
  onShowMobileSessions,
  onToggleSessionSidebar,
  onOpenAppSidebar,
}: ChatHeaderProps) {
  const { t } = useI18n();
  const statusIcon = !connected ? (
    <WifiOff className="h-3.5 w-3.5" />
  ) : (
    <Wifi className="h-3.5 w-3.5" />
  );
  const connectionLabel = connected
    ? t("Local agent ready")
    : t("Connecting to local agent");

  return (
    <header className="flex shrink-0 items-center justify-between gap-3 border-b border-border/60 bg-background/95 px-3 py-2">
      <div className="flex min-w-0 items-center gap-2">
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          onClick={onShowMobileSessions}
          className="text-muted-foreground hover:text-foreground md:hidden"
          title={t("Show sessions")}
          aria-label={t("Show sessions")}
        >
          <PanelLeftOpen className="h-4 w-4" />
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          onClick={onToggleSessionSidebar}
          className="hidden text-muted-foreground hover:text-foreground md:inline-flex"
          title={showSessionSidebar ? t("Hide sessions") : t("Show sessions")}
          aria-label={showSessionSidebar ? t("Hide sessions") : t("Show sessions")}
        >
          {showSessionSidebar ? (
            <PanelLeftClose className="h-4 w-4" />
          ) : (
            <PanelLeftOpen className="h-4 w-4" />
          )}
        </Button>
        <div className="flex h-10 w-12 shrink-0 items-center justify-center text-muted-foreground">
          <SessionHostLogo
            agentId={selectedAgent}
            agentLabel={agentLabel}
            profileId={profileId}
            profileLabel={profileLabel}
            providerId={providerId}
            providerLabel={providerLabel}
            size="md"
          />
        </div>
        <div className="min-w-0">
          <div className="truncate text-sm font-medium text-foreground">
            {routeLabel}
          </div>
          {(workspacePath || headerSessionLabel || sessionId) && (
            <div className="flex min-w-0 items-center gap-1.5 font-mono text-[10px] text-muted-foreground/60">
              {workspacePath && (
                <span
                  className="min-w-0 max-w-[18rem] truncate text-muted-foreground/70"
                  title={workspacePath}
                >
                  {workspacePath}
                </span>
              )}
              {headerSessionLabel && (
                <span className="truncate">{headerSessionLabel}</span>
              )}
              {sessionId && (
                <span className="truncate text-muted-foreground/40">
                  {shortSessionId(sessionId)}
                </span>
              )}
            </div>
          )}
        </div>
      </div>
      <div className="flex shrink-0 items-center gap-1.5">
        <div
          className={
            connected
              ? "flex shrink-0 items-center gap-1.5 rounded-md px-2 py-1 font-mono text-[10px] text-emerald-400/80"
              : "flex shrink-0 items-center gap-1.5 rounded-md px-2 py-1 font-mono text-[10px] text-muted-foreground/60"
          }
          title={connectionLabel}
        >
          {statusIcon}
          <span className="hidden sm:inline">{connectionLabel}</span>
        </div>
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          onClick={onOpenAppSidebar}
          className="text-muted-foreground hover:text-foreground md:hidden"
          title={t("Show navigation")}
          aria-label={t("Show navigation")}
        >
          <Menu className="h-4 w-4" />
        </Button>
      </div>
    </header>
  );
}
