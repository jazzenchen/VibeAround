import { useCallback, useEffect, useState } from "react";
import { LayoutGrid, Rows3, Minimize2, MessageSquare, Plus, X } from "lucide-react";
import type { ViewMode, TerminalGroup, TerminalSession, TerminalStatus, ToolType } from "@/lib/terminal-types";
import { getGroupColor, TOOL_OPTIONS } from "@/lib/terminal-types";
import { TerminalPanel } from "@/components/TerminalPanel";
import { ChatView } from "@/components/chat";
import { getSessions, createSession, deleteSession, type SessionListItem } from "@/api/sessions";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";

const DEFAULT_GROUP_ID = "default";

function sessionToName(tool: string): string {
  const t = tool.toLowerCase();
  if (t === "claude") return "Claude";
  if (t === "gemini") return "Gemini";
  if (t === "codex") return "Codex";
  return "Terminal";
}

function mapApiStatus(s: string): TerminalStatus {
  if (s === "running") return "running";
  if (s === "exited") return "stopped";
  return "idle";
}

function mapApiTool(s: string): ToolType {
  const t = s.toLowerCase();
  if (t === "claude" || t === "gemini" || t === "codex" || t === "generic") return t as ToolType;
  return "generic";
}

function sessionListItemToSession(item: SessionListItem): TerminalSession {
  return {
    id: item.session_id,
    name: sessionToName(item.tool),
    group: DEFAULT_GROUP_ID,
    tool: mapApiTool(item.tool),
    status: mapApiStatus(item.status),
    command: item.tool,
    cwd: item.project_path ?? "—",
    startedAt: item.created_at * 1000,
    createdAt: item.created_at,
  };
}

type AppPage = "terminal" | "chat";

const DEFAULT_GROUPS: TerminalGroup[] = [
  { id: DEFAULT_GROUP_ID, label: "CLI", color: "#64748b", sessions: [] },
];

function App() {
  const [page, setPage] = useState<AppPage>("terminal");
  const [viewMode, setViewMode] = useState<ViewMode>("tabs");
  const [groups, setGroups] = useState<TerminalGroup[]>(DEFAULT_GROUPS);
  const [activeTabId, setActiveTabId] = useState<string | null>(null);
  const [maximizedSession, setMaximizedSession] = useState<string | null>(null);
  const [sessionsLoading, setSessionsLoading] = useState(true);
  const [pingMs, setPingMs] = useState<number | null>(null);

  // Ping backend every 10s to show network latency next to "connected".
  useEffect(() => {
    if (typeof window === "undefined") return;
    const measurePing = async () => {
      const origin = window.location.origin;
      const start = performance.now();
      try {
        await fetch(origin + "/", { method: "HEAD", cache: "no-store" });
        setPingMs(Math.round(performance.now() - start));
      } catch {
        setPingMs(null);
      }
    };
    measurePing();
    const interval = setInterval(measurePing, 10_000);
    return () => clearInterval(interval);
  }, []);

  // Load sessions from backend on mount and when returning to terminal page.
  useEffect(() => {
    if (page !== "terminal") return;
    setSessionsLoading(true);
    getSessions()
      .then((list) => {
        const sessions = list.map(sessionListItemToSession);
        setGroups((prev) => {
          const g = prev.find((x) => x.id === DEFAULT_GROUP_ID) ?? DEFAULT_GROUPS[0];
          return [{ ...g, sessions }];
        });
        if (sessions.length > 0) {
          setActiveTabId((prev) =>
            prev && sessions.some((s) => s.id === prev) ? prev : sessions[0].id
          );
        } else {
          setActiveTabId(null);
        }
      })
      .catch((e) => console.error("[VibeAround] getSessions:", e))
      .finally(() => setSessionsLoading(false));
  }, [page]);

  const closeSession = useCallback(async (sessionId: string) => {
    try {
      await deleteSession(sessionId);
      setGroups((prev) =>
        prev.map((g) => ({
          ...g,
          sessions: g.sessions.filter((s) => s.id !== sessionId),
        }))
      );
      setActiveTabId((prev) => {
        if (prev !== sessionId) return prev;
        const remaining = groups.flatMap((g) => g.sessions).filter((s) => s.id !== sessionId);
        return remaining[0]?.id ?? null;
      });
      setMaximizedSession((m) => (m === sessionId ? null : m));
    } catch (e) {
      console.error("[VibeAround] deleteSession:", e);
    }
  }, [groups]);

  const handleAddCli = useCallback(async (tool: ToolType) => {
    try {
      const res = await createSession({ tool });
      const session = sessionListItemToSession({
        session_id: res.session_id,
        tool: res.tool,
        status: "running",
        created_at: res.created_at,
        project_path: res.project_path,
      });
      setGroups((prev) =>
        prev.map((g) =>
          g.id === DEFAULT_GROUP_ID
            ? { ...g, sessions: [...g.sessions, session] }
            : g
        )
      );
      setActiveTabId(session.id);
    } catch (e) {
      console.error("[VibeAround] createSession:", e);
    }
  }, []);

  const toggleMaximize = useCallback((sessionId: string) => {
    setMaximizedSession((prev) => (prev === sessionId ? null : sessionId));
  }, []);

  /** Update session tool and status from backend (PTY run state: running / exited). Drives CLI style. */
  const setSessionState = useCallback(
    (sessionId: string, tool: ToolType, status: TerminalStatus) => {
      setGroups((prev) =>
        prev.map((g) => ({
          ...g,
          sessions: g.sessions.map((s) =>
            s.id === sessionId ? { ...s, tool, status } : s
          ),
        }))
      );
    },
    []
  );

  const activeSession = groups
    .flatMap((g) => g.sessions)
    .find((s) => s.id === activeTabId);
  const maximizedSessionData = maximizedSession
    ? groups.flatMap((g) => g.sessions).find((s) => s.id === maximizedSession)
    : null;
  const totalSessions = groups.reduce((sum, g) => sum + g.sessions.length, 0);
  const runningSessions = groups.reduce(
    (sum, g) => sum + g.sessions.filter((s) => s.status === "running").length,
    0
  );

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden bg-background">
      {/* Header: VibeAround + view toggle. No tab bar here. */}
      <header
        className="flex items-center justify-between px-3 py-1.5 shrink-0"
        style={{
          backgroundColor: "oklch(0.10 0.005 260)",
          borderBottom: "1px solid oklch(0.20 0.01 260)",
        }}
      >
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2">
            <span className="inline-block h-2 w-2 rounded-sm bg-primary" />
            <h1 className="text-xs font-semibold text-foreground font-mono tracking-tight">
              VibeAround
            </h1>
          </div>
          <span className="text-[9px] text-muted-foreground/40 font-mono">
            v0.1.0
          </span>
          <ToggleGroup
            type="single"
            value={page}
            onValueChange={(v) => v && setPage(v as AppPage)}
            className="flex items-center gap-0.5 rounded-md p-0.5 border-l border-border/20 ml-3 pl-3 font-mono text-xs"
            style={{ backgroundColor: "oklch(0.15 0.005 260)" }}
          >
            <ToggleGroupItem value="terminal" aria-label="Terminal" className="rounded px-2 py-1 gap-1.5 data-[state=on]:bg-primary/15 data-[state=on]:text-primary text-muted-foreground/50 hover:text-foreground">
              <Rows3 className="h-3 w-3" />
              Terminal
            </ToggleGroupItem>
            <ToggleGroupItem value="chat" aria-label="Chat" className="rounded px-2 py-1 gap-1.5 data-[state=on]:bg-primary/15 data-[state=on]:text-primary text-muted-foreground/50 hover:text-foreground">
              <MessageSquare className="h-3 w-3" />
              Chat
            </ToggleGroupItem>
          </ToggleGroup>
          <div className={`hidden items-center gap-3 border-l border-border/20 pl-3 sm:flex ${page === "terminal" ? "" : "hidden"}`}>
            <span className="text-[10px] text-muted-foreground/50 font-mono">
              {runningSessions}/{totalSessions} active
            </span>
            <span className="text-[10px] text-emerald-400/80 font-mono flex items-center gap-1.5">
              <span className="inline-block h-1.5 w-1.5 rounded-full bg-emerald-400 animate-pulse" />
              connected
              {pingMs !== null ? (
                <span className="text-muted-foreground/70">· {pingMs} ms</span>
              ) : (
                <span className="text-muted-foreground/50">· — ms</span>
              )}
            </span>
          </div>
        </div>
        <ToggleGroup
          type="single"
          value={viewMode}
          onValueChange={(v) => v && setViewMode(v as ViewMode)}
          className={`flex items-center gap-0.5 rounded-md p-0.5 font-mono text-xs ${page === "terminal" ? "" : "hidden"}`}
          style={{ backgroundColor: "oklch(0.15 0.005 260)" }}
        >
          <ToggleGroupItem value="tabs" aria-label="Tab view" className="rounded px-2 py-1 data-[state=on]:bg-primary/15 data-[state=on]:text-primary text-muted-foreground/50 hover:text-foreground">
            <Rows3 className="h-3 w-3" />
          </ToggleGroupItem>
          <ToggleGroupItem value="grid" aria-label="Grid view" className="rounded px-2 py-1 data-[state=on]:bg-primary/15 data-[state=on]:text-primary text-muted-foreground/50 hover:text-foreground">
            <LayoutGrid className="h-3 w-3" />
          </ToggleGroupItem>
        </ToggleGroup>
      </header>

      {/* Tab bar: only in tabs view and terminal page. Hidden in grid (flat) view. */}
      {page === "terminal" && viewMode === "tabs" && (
        <nav
          className="flex items-stretch overflow-x-auto scrollbar-none shrink-0"
          style={{ backgroundColor: "oklch(0.12 0.005 260)" }}
        >
          <div className="flex items-stretch min-w-0">
            {groups.map((group) => {
              const gc = getGroupColor(group.color);
              return (
                <div key={group.id} className="flex items-stretch">
                  <div className="relative flex items-center shrink-0">
                    <button
                      className="flex items-center gap-1.5 px-2.5 py-2 transition-colors shrink-0"
                      style={{
                        backgroundColor: `${gc.bg}12`,
                      }}
                    >
                      <span
                        className="inline-block h-1.5 w-1.5 rounded-full shrink-0"
                        style={{ backgroundColor: gc.bg }}
                      />
                      <span
                        className="text-[10px] font-semibold font-mono whitespace-nowrap"
                        style={{ color: gc.bg }}
                      >
                        {group.label}
                      </span>
                    </button>
                    <div
                      className="absolute bottom-0 left-0 right-0 h-[2px]"
                      style={{ backgroundColor: gc.bg }}
                    />
                  </div>
                  {group.sessions.map((session) => {
                    const isActive = session.id === activeTabId;
                    return (
                      <div
                        key={session.id}
                        className="relative flex items-center group gap-2"
                      >
                        <button
                          onClick={() => {
                            setActiveTabId(session.id);
                            setMaximizedSession(null);
                          }}
                          className={`relative flex items-center gap-1.5 pl-2.5 pr-5 py-2 text-[11px] font-mono transition-all whitespace-nowrap ${
                            isActive
                              ? "text-foreground"
                              : "text-muted-foreground/50 hover:text-muted-foreground/80"
                          }`}
                          style={{
                            backgroundColor: isActive ? `${gc.bg}15` : "transparent",
                          }}
                        >
                          <span
                            className={`inline-block h-1.5 w-1.5 rounded-full shrink-0 ${
                              session.status === "running" ? "animate-pulse" : ""
                            }`}
                            style={{
                              backgroundColor:
                                session.status === "running"
                                  ? "#4ade80"
                                  : session.status === "idle"
                                    ? "#fbbf24"
                                    : "#64748b",
                            }}
                          />
                          {session.name}
                        </button>
                        <span
                          className="absolute bottom-0 left-0 right-0 h-[2px] pointer-events-none"
                          style={{
                            backgroundColor: isActive ? gc.bg : `${gc.bg}50`,
                          }}
                        />
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon-xs"
                          onClick={(e) => {
                            e.stopPropagation();
                            closeSession(session.id);
                          }}
                          className="absolute p-0 h-4 w-4 right-1 top-1/2 -translate-y-1/2 text-muted-foreground/30 hover:text-foreground opacity-0 group-hover:opacity-100"
                          title="Close session"
                          aria-label="Close session"
                        >
                          <X className="h-2.5 w-2.5" />
                        </Button>
                      </div>
                    );
                  })}
                  <div
                    className="flex items-center shrink-0 px-1"
                    style={{ borderLeft: "1px solid oklch(0.25 0.01 260)" }}
                  />
                </div>
              );
            })}
            <div className="relative flex items-center shrink-0 pl-1 overflow-visible">
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button
                    variant="outline"
                    size="sm"
                    className="h-auto gap-1 px-2.5 py-2 text-[11px] font-mono text-muted-foreground hover:text-foreground border-transparent hover:border-border/30"
                    aria-label="Add CLI session"
                  >
                    <Plus className="h-3.5 w-3.5" />
                    Add CLI
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent className="min-w-[140px] font-mono text-xs" align="start">
                  <DropdownMenuLabel className="text-[10px] uppercase tracking-wider text-muted-foreground">
                    New session
                  </DropdownMenuLabel>
                  {TOOL_OPTIONS.map((tool) => (
                    <DropdownMenuItem
                      key={tool}
                      onSelect={() => handleAddCli(tool)}
                      className="capitalize"
                    >
                      {sessionToName(tool)}
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
          </div>
        </nav>
      )}

      {/* Main: Chat page or terminal (one panel / maximized / active tab / grid). */}
      <main className="flex-1 min-h-0 overflow-hidden">
        {page === "chat" ? (
          <ChatView />
        ) : maximizedSessionData ? (
          <div className="h-full p-2">
            <TerminalPanel
              session={maximizedSessionData}
              isActive
              isMaximized
              viewMode={viewMode}
              onToggleMaximize={() =>
                toggleMaximize(maximizedSessionData.id)
              }
              onClose={() => closeSession(maximizedSessionData.id)}
              onSessionState={(tool, status) =>
                setSessionState(maximizedSessionData.id, tool, status)
              }
            />
          </div>
        ) : viewMode === "tabs" ? (
          <div className="h-full p-2">
            {activeSession ? (
              <TerminalPanel
                session={activeSession}
                isActive
                viewMode={viewMode}
                onToggleMaximize={() => toggleMaximize(activeSession.id)}
                onClose={() => closeSession(activeSession.id)}
                onSessionState={(tool, status) =>
                  setSessionState(activeSession.id, tool, status)
                }
              />
            ) : sessionsLoading ? (
              <div className="flex h-full items-center justify-center">
                <p className="text-sm text-muted-foreground/40 font-mono">Loading sessions…</p>
              </div>
            ) : (
              <div className="flex h-full flex-col items-center justify-center gap-3">
                <p className="text-sm text-muted-foreground/40 font-mono">
                  No sessions yet. Add a CLI to start.
                </p>
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button variant="outline" size="sm" className="gap-1.5 font-mono text-xs text-primary border-primary/30 hover:bg-primary/10">
                      <Plus className="h-3.5 w-3.5" />
                      Add CLI
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent className="min-w-[140px] font-mono text-xs" align="center">
                    <DropdownMenuLabel className="text-[10px] uppercase tracking-wider text-muted-foreground">
                      New session
                    </DropdownMenuLabel>
                    {TOOL_OPTIONS.map((tool) => (
                      <DropdownMenuItem
                        key={tool}
                        onSelect={() => handleAddCli(tool)}
                        className="capitalize"
                      >
                        {sessionToName(tool)}
                      </DropdownMenuItem>
                    ))}
                  </DropdownMenuContent>
                </DropdownMenu>
              </div>
            )}
          </div>
        ) : (
          <GridContent
            groups={groups}
            viewMode={viewMode}
            toggleMaximize={toggleMaximize}
            setSessionState={setSessionState}
            closeSession={closeSession}
          />
        )}
      </main>

      {/* Footer */}
      <footer
        className="flex items-center justify-between px-3 py-1 shrink-0"
        style={{
          backgroundColor: "oklch(0.09 0.005 260)",
          borderTop: "1px solid oklch(0.18 0.01 260)",
        }}
      >
        <div className="flex items-center gap-3">
          <span className="text-[10px] text-muted-foreground/40 font-mono truncate max-w-[180px]" title="WebSocket follows page host (tunnel works on phone)">
            WS: {typeof window !== "undefined" ? (window.location.protocol === "https:" ? "wss" : "ws") + "://" + window.location.host + "/ws" : "—"}
          </span>
          <span className="text-[10px] text-muted-foreground/30 font-mono">
            Tunnel: — (see desktop tray)
          </span>
        </div>
        <div className="flex items-center gap-3">
          {maximizedSession && (
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setMaximizedSession(null)}
              className="gap-1 text-[10px] font-mono text-primary/60 hover:text-primary h-auto py-1 px-2"
            >
              <Minimize2 className="h-2.5 w-2.5" />
              Exit Maximized
            </Button>
          )}
          <span className="text-[10px] text-muted-foreground/30 font-mono uppercase">
            {viewMode}
          </span>
          <span className="text-[10px] text-muted-foreground/40 font-mono">
            {runningSessions} proc
          </span>
        </div>
      </footer>
    </div>
  );
}

function GridContent({
  groups,
  viewMode,
  toggleMaximize,
  setSessionState,
  closeSession,
}: {
  groups: TerminalGroup[];
  viewMode: ViewMode;
  toggleMaximize: (id: string) => void;
  setSessionState: (sessionId: string, tool: ToolType, status: TerminalStatus) => void;
  closeSession: (sessionId: string) => void;
}) {
  return (
    <div
      className="h-full overflow-y-auto p-3 pr-5 scrollbar-thin"
      style={{ overscrollBehavior: "contain" }}
    >
      <div className="flex flex-col gap-6 pb-6">
        {groups.map((group) => {
          const gc = getGroupColor(group.color);
          return (
            <section key={group.id}>
              <div className="mb-2 flex items-center gap-2">
                <span
                  className="inline-block h-2 w-2 rounded-full shrink-0"
                  style={{ backgroundColor: gc.bg }}
                />
                <h2
                  className="text-[11px] font-bold uppercase tracking-wider font-mono"
                  style={{ color: gc.bg }}
                >
                  {group.label}
                </h2>
                <span className="text-[10px] text-muted-foreground/30 font-mono">
                  {group.sessions.filter((s) => s.status === "running").length}/
                  {group.sessions.length}
                </span>
                <div
                  className="flex-1 h-px"
                  style={{ backgroundColor: `${gc.bg}25` }}
                />
              </div>
              <div className="grid gap-4 grid-cols-1 lg:grid-cols-2">
                {group.sessions.map((session) => (
                  <div key={session.id} className="h-[320px] lg:h-[360px]">
                    <TerminalPanel
                      session={session}
                      isActive
                      viewMode={viewMode}
                      onToggleMaximize={() => toggleMaximize(session.id)}
                      onClose={() => closeSession(session.id)}
                      onSessionState={(tool, status) =>
                        setSessionState(session.id, tool, status)
                      }
                    />
                  </div>
                ))}
              </div>
            </section>
          );
        })}
      </div>
    </div>
  );
}

export default App;
