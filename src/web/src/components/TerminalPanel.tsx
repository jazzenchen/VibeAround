"use client";

import { Maximize2, Minimize2, X } from "lucide-react";
import { useCallback, useState } from "react";
import type { TerminalSession, TerminalStatus, ToolType, ViewMode } from "@/lib/terminal-types";
import { toolThemes } from "@/lib/terminal-types";
import { TerminalView } from "./TerminalView";
import { MobileInputBar } from "./MobileInputBar";
import { Button } from "@/components/ui/button";

interface TerminalPanelProps {
  session: TerminalSession;
  isActive: boolean;
  isMaximized?: boolean;
  viewMode?: ViewMode;
  onToggleMaximize?: () => void;
  onClose?: () => void;
  /** Called when backend sends PTY run state (running / exited). Updates tool + status for styling. */
  onSessionState?: (tool: ToolType, status: TerminalStatus) => void;
}

const statusConfig = {
  running: { label: "RUNNING", pulse: true },
  idle: { label: "IDLE", pulse: false },
  stopped: { label: "STOPPED", pulse: false },
  error: { label: "ERROR", pulse: true },
} as const;

export function TerminalPanel({
  session,
  isActive,
  isMaximized = false,
  viewMode,
  onToggleMaximize,
  onClose,
  onSessionState,
}: TerminalPanelProps) {
  const theme = toolThemes[session.tool];
  const status = statusConfig[session.status];

  // Mobile: hold sendInput callback from TerminalView's WebSocket.
  const [isMobile] = useState(() =>
    typeof navigator !== "undefined" && /iPhone|iPad|iPod|Android/i.test(navigator.userAgent)
  );
  const [sendInput, setSendInput] = useState<((data: string) => void) | null>(null);

  const handleSendInputReady = useCallback((fn: (data: string) => void) => {
    // Wrap in arrow so React setState doesn't call fn as an updater.
    setSendInput(() => fn);
  }, []);

  return (
    <div
      className="flex h-full flex-col overflow-hidden rounded-lg"
      style={{
        border: `1px solid ${theme.borderColor}`,
        boxShadow: `0 0 0 1px ${theme.borderColor}, inset 0 1px 0 0 ${theme.accent}15`,
      }}
    >
      <div
        className="flex items-center justify-between px-3 py-1.5"
        style={{
          backgroundColor: theme.headerBg,
          borderBottom: `1px solid ${theme.borderColor}`,
        }}
      >
        <div className="flex items-center gap-2">
          <div
            className="h-3 w-0.5 rounded-full"
            style={{ backgroundColor: theme.accent }}
          />
          <div
            className="rounded px-1.5 py-0.5 text-[9px] font-bold uppercase tracking-wider font-mono"
            style={{
              backgroundColor: `${theme.accent}20`,
              color: theme.accent,
            }}
          >
            {theme.label}
          </div>
          <div className="text-xs font-medium text-foreground/90 font-mono">
            {session.name}
          </div>
          <div className="flex items-center gap-1.5 pl-2">
            <div
              className={`h-1.5 w-1.5 shrink-0 rounded-full self-center ${status.pulse ? "animate-pulse" : ""}`}
              style={{
                backgroundColor:
                  session.status === "running"
                    ? "#4ade80"
                    : session.status === "idle"
                      ? "#fbbf24"
                      : session.status === "error"
                        ? "#f87171"
                        : "#64748b",
              }}
            />
            <div className="text-[9px] h-1.5 font-mono text-muted-foreground/50 uppercase leading-none">
              {status.label}
            </div>
          </div>
        </div>
        <div className="flex items-center gap-1.5">
          <span className="hidden text-[10px] text-muted-foreground/30 font-mono md:inline truncate max-w-32">
            {session.cwd}
          </span>
          {onClose && (
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              onClick={onClose}
              className="text-muted-foreground/40 hover:text-foreground"
              title="Close session"
              aria-label="Close session"
            >
              <X className="h-3 w-3" />
            </Button>
          )}
          {onToggleMaximize && (
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={onToggleMaximize}
              className="text-muted-foreground/40 hover:text-foreground"
              title={isMaximized ? "Restore" : "Maximize"}
              aria-label={isMaximized ? "Restore panel" : "Maximize panel"}
            >
              {isMaximized ? (
                <Minimize2 className="h-3 w-3" />
              ) : (
                <Maximize2 className="h-3 w-3" />
              )}
            </Button>
          )}
        </div>
      </div>
      <div
        className="flex-1 min-h-0 overflow-hidden"
        style={{ overscrollBehavior: "contain" }}
      >
        <TerminalView session={session} isActive={isActive} viewMode={viewMode} onSessionState={onSessionState} onSendInputReady={handleSendInputReady} />
      </div>
      {/* Mobile shortcut bar + prompt overlay */}
      {isMobile && sendInput && (
        <MobileInputBar sendInput={sendInput} />
      )}
      <div
        className="flex items-center gap-2 px-3 py-1"
        style={{
          backgroundColor: theme.headerBg,
          borderTop: `1px solid ${theme.borderColor}`,
        }}
      >
        <span
          className="text-[9px] font-mono"
          style={{ color: `${theme.accent}80` }}
        >
          $
        </span>
        <span className="text-[10px] text-muted-foreground/40 font-mono truncate">
          {session.command}
        </span>
      </div>
    </div>
  );
}
