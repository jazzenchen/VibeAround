"use client";

import { useCallback, useEffect, useRef } from "react";
import type { Terminal as XTermTerminal } from "@xterm/xterm";
import type { FitAddon } from "@xterm/addon-fit";
import type { TerminalSession, TerminalStatus, ToolType, ViewMode } from "@/lib/terminal-types";
import { toolThemes } from "@/lib/terminal-types";
import { getWebSocketUrl } from "@/lib/ws-url";

/** "dom" = default DOM renderer; "canvas" = Canvas addon; "webgl" = WebGL addon (GPU). */
const XTERM_RENDERER: "dom" | "canvas" | "webgl" = "webgl";

/** Backend sends this when PTY run state changes (child try_wait). */
interface SessionStateMessage {
  type: "running" | "exited";
  tool: "generic" | "claude" | "gemini" | "codex";
  exit_code?: number;
}

function mapTool(t: SessionStateMessage["tool"]): ToolType {
  return t;
}

interface TerminalViewProps {
  session: TerminalSession;
  isActive: boolean;
  viewMode?: ViewMode;
  onSessionState?: (tool: ToolType, status: TerminalStatus) => void;
}

export function TerminalView({ session, isActive, viewMode, onSessionState }: TerminalViewProps) {
  /** Element passed to term.open(); ResizeObserver watches this so fit uses the same box. */
  const fitTargetRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<XTermTerminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const cleanupRef = useRef<(() => void) | null>(null);
  const initializedRef = useRef(false);
  const onSessionStateRef = useRef(onSessionState);
  onSessionStateRef.current = onSessionState;

  const theme = toolThemes[session.tool];

  const themeOption = useCallback(
    (t: typeof theme) => ({
      background: t.bg,
      foreground: "#c8c8d8",
      cursor: t.cursorColor,
      cursorAccent: t.bg,
      selectionBackground: t.selectionBg,
      selectionForeground: "#ffffff",
      black: "#1a1a2e",
      red: "#f87171",
      green: "#4ade80",
      yellow: "#fbbf24",
      blue: "#60a5fa",
      magenta: "#c084fc",
      cyan: "#4fd1c5",
      white: "#c8c8d8",
      brightBlack: "#4a4a6a",
      brightRed: "#fca5a5",
      brightGreen: "#86efac",
      brightYellow: "#fde68a",
      brightBlue: "#93c5fd",
      brightMagenta: "#d8b4fe",
      brightCyan: "#5eead4",
      brightWhite: "#f0f0f8",
    }),
    []
  );

  const initTerminal = useCallback(async () => {
    if (!fitTargetRef.current || initializedRef.current) return;
    initializedRef.current = true;

    // Simple mobile detection: disable stdin on touch devices to avoid virtual keyboard / focus issues
    const isMobile = /iPhone|iPad|iPod|Android/i.test(navigator.userAgent);

    const { Terminal } = await import("@xterm/xterm");
    const { FitAddon } = await import("@xterm/addon-fit");
    await import("@xterm/xterm/css/xterm.css");

    const fitAddon = new FitAddon();
    fitAddonRef.current = fitAddon;

    const term = new Terminal({
      cursorBlink: true,
      cursorStyle: "bar",
      fontSize: 12,
      fontFamily: "JetBrains Mono, ui-monospace, monospace",
      lineHeight: XTERM_RENDERER !== "dom" ? 1.35 : 1.1,
      theme: themeOption(theme),
      scrollback: 5000,
      convertEol: true,
      allowProposedApi: true,
      disableStdin: isMobile,
    });

    term.loadAddon(fitAddon);
    term.open(fitTargetRef.current);
    if (XTERM_RENDERER === "canvas") {
      const { CanvasAddon } = await import("@xterm/addon-canvas");
      term.loadAddon(new CanvasAddon());
    } else if (XTERM_RENDERER === "webgl") {
      const { WebglAddon } = await import("@xterm/addon-webgl");
      term.loadAddon(new WebglAddon());
    }
    termRef.current = term;
    // Sync fit once after mount so cols/rows match the visible area (layout may not be final yet).
    const syncFit = () => {
      try {
        fitAddon.fit();
      } catch {
        /* ignore */
      }
    };
    syncFit();
    requestAnimationFrame(() => {
      syncFit();
      setTimeout(syncFit, 0);
    });

    term.writeln("VibeAround Web Dashboard — connecting…");

    const wsUrl = getWebSocketUrl(`/ws?session_id=${encodeURIComponent(session.id)}`);
    const ws = new WebSocket(wsUrl);
    wsRef.current = ws;
    let dumpReceived = false;

    // Send cols/rows to PTY (already account for padding: fit target is inner div inside padded outer).
    const sendResize = () => {
      try {
        fitAddon.fit();
        if (ws.readyState === WebSocket.OPEN) {
          const cols = term.cols-1;
          const rows = term.rows;
          ws.send(JSON.stringify({ type: "resize", cols, rows }));
        }
      } catch {
        /* ignore */
      }
    };

    ws.onopen = () => {
      term.writeln("\r\nConnected. Receiving history…\r\n");
      sendResize();
    };

    // Backend sends: (1) one Binary = dump (history), (2) one Text = state, (3) then Binary = live. On reconnect, clear before dump.
    ws.onmessage = (ev) => {
      const data = ev.data;
      if (data instanceof ArrayBuffer) {
        const bytes = new Uint8Array(data);
        if (!dumpReceived) {
          term.reset();
          dumpReceived = true;
        }
        term.write(bytes);
        return;
      }
      if (data instanceof Blob) {
        data.arrayBuffer().then((buf) => {
          const bytes = new Uint8Array(buf);
          if (!dumpReceived) {
            term.reset();
            dumpReceived = true;
          }
          term.write(bytes);
        });
        return;
      }
      if (typeof data === "string") {
        try {
          const msg = JSON.parse(data) as SessionStateMessage;
          if (msg.type === "running" || msg.type === "exited") {
            onSessionStateRef.current?.(mapTool(msg.tool), msg.type === "running" ? "running" : msg.exit_code === 0 ? "stopped" : "error");
            return;
          }
        } catch {
          /* not JSON or not session_state */
        }
        if (!dumpReceived) {
          term.reset();
          dumpReceived = true;
        }
        term.write(data);
        return;
      }
    };

    ws.onclose = () => {
      term.writeln("\r\n\r\n[Connection closed. Refresh to reconnect.]");
    };

    ws.onerror = () => {
      term.writeln("\r\n\r\n[WebSocket error. Is the app running?]");
    };

    const dispose = term.onData((data) => {
      if (ws.readyState === WebSocket.OPEN) ws.send(data);
    });

    cleanupRef.current = () => {
      dispose.dispose();
      ws.close();
    };
  }, [session.id]);

  useEffect(() => {
    initTerminal();
    return () => {
      cleanupRef.current?.();
      cleanupRef.current = null;
      if (termRef.current) {
        termRef.current.dispose();
        termRef.current = null;
      }
      if (wsRef.current) {
        wsRef.current.close();
        wsRef.current = null;
      }
      initializedRef.current = false;
    };
  }, [initTerminal]);

  // Update xterm theme and cursorBlink when session.tool/session.status changes, without reconnecting WS.
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    term.options.theme = themeOption(theme);
    term.options.cursorBlink = session.status === "running";
  }, [theme, themeOption, session.status]);

  useEffect(() => {
    if (isActive && fitAddonRef.current) {
      const t = setTimeout(() => {
        try {
          fitAddonRef.current?.fit();
        } catch {
          /* ignore */
        }
      }, 100);
      return () => clearTimeout(t);
    }
  }, [isActive]);

  // Shared: fit terminal to fit target, then send cols/rows to PTY (used on resize and viewMode change).
  const fitAndSendResize = () => {
    const term = termRef.current;
    const fitAddon = fitAddonRef.current;
    const ws = wsRef.current;
    try {
      fitAddon?.fit();
      if (ws?.readyState === WebSocket.OPEN && term) {
        ws.send(JSON.stringify({ type: "resize", cols: term.cols - 1, rows: term.rows }));
      }
    } catch {
      /* ignore */
    }
  };

  // When switching to grid (or tabs), layout changes; run fit + send resize after a short delay.
  useEffect(() => {
    if (!viewMode) return;
    const t = setTimeout(fitAndSendResize, 100);
    return () => clearTimeout(t);
  }, [viewMode]);

  // Resize: observe fit target and run fit + send resize on size change (and window resize).
  useEffect(() => {
    window.addEventListener("resize", fitAndSendResize);
    const el = fitTargetRef.current;
    if (!el) return () => window.removeEventListener("resize", fitAndSendResize);
    const ro = new ResizeObserver(fitAndSendResize);
    ro.observe(el);
    return () => {
      window.removeEventListener("resize", fitAndSendResize);
      ro.disconnect();
    };
  }, []);

  return (
    <div
      className="h-full w-full box-border p-2"
      style={{
        backgroundColor: theme.bg,
        overscrollBehavior: "contain",
      }}
    >
      <div ref={fitTargetRef} className="h-full w-full" />
    </div>
  );
}
