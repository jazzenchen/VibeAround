"use client";

import type { CSSProperties } from "react";
import { useState } from "react";
import {
  Archive,
  ChevronDown,
  ChevronRight,
  Folder,
  Loader2,
  PlusCircle,
  RefreshCw,
} from "lucide-react";
import type { AgentInfo, LaunchSessionInfo } from "@va/client";
import { useI18n } from "@va/i18n";

import { BrandIcon } from "@/components/brand-icon";
import { cn } from "@/lib/utils";
import type { ChatSessionSelection } from "./chatTypes";
import {
  ALL_AGENTS_FILTER,
  chatSessionKey,
  type ChatSessionWorkspaceGroup,
} from "./chatSessionModel";
import {
  formatSessionUpdatedAt,
  sessionProfileTypeLabel,
  sortSessionsByUpdatedAt,
  workspaceLabel,
} from "./chatSessionDisplay";
import { SessionHostLogo } from "./SessionHostLogo";

const SESSION_PREVIEW_LIMIT = 5;

interface ChatSessionSidebarProps {
  workspaceGroups: ChatSessionWorkspaceGroup[];
  agents: AgentInfo[];
  selectedAgentFilter: string;
  activeAgentId: string;
  variant?: "desktop" | "mobile";
  className?: string;
  style?: CSSProperties;
  sessionsLoading?: boolean;
  loadingSessionId?: string;
  loadingSessionKeys?: ReadonlySet<string>;
  archivingSessionId?: string;
  sessionSelection: ChatSessionSelection;
  onSyncSessions: () => void;
  onAgentFilterChange: (agentId: string) => void;
  onSessionChange: (selection: ChatSessionSelection, session?: LaunchSessionInfo) => void;
  onArchiveSession: (session: LaunchSessionInfo) => void;
}

function sessionButtonClass(active: boolean, archived = false) {
  return cn(
    "group flex w-full items-start gap-2 rounded-md px-2 py-2 text-left text-sm transition-colors",
    active && archived
      ? "bg-primary/5 text-muted-foreground/65 ring-1 ring-primary/15"
      : active
        ? "bg-primary/10 text-primary"
        : archived
          ? "text-muted-foreground/40 hover:bg-muted/40 hover:text-muted-foreground/65"
          : "text-muted-foreground hover:bg-muted/70 hover:text-foreground",
  );
}

export function ChatSessionSidebar({
  workspaceGroups,
  agents,
  selectedAgentFilter,
  activeAgentId,
  variant = "desktop",
  className,
  style,
  sessionsLoading = false,
  loadingSessionId,
  loadingSessionKeys,
  archivingSessionId,
  sessionSelection,
  onSyncSessions,
  onAgentFilterChange,
  onSessionChange,
  onArchiveSession,
}: ChatSessionSidebarProps) {
  const { t } = useI18n();
  const [collapsedWorkspaces, setCollapsedWorkspaces] = useState<Record<string, boolean>>({});
  const [expandedSessionLists, setExpandedSessionLists] = useState<Record<string, boolean>>({});

  const toggleWorkspace = (workspace: string) => {
    setCollapsedWorkspaces((prev) => ({
      ...prev,
      [workspace]: !prev[workspace],
    }));
  };

  const toggleSessionList = (workspace: string) => {
    setExpandedSessionLists((prev) => ({
      ...prev,
      [workspace]: !prev[workspace],
    }));
  };

  const agentLabel = (agentId: string) =>
    agents.find((agent) => agent.id === agentId)?.name ?? agentId;

  return (
    <aside
      className={cn(
        "h-full shrink-0 flex-col border-r border-border bg-muted/20",
        variant === "mobile" ? "flex w-full" : "hidden w-64 md:flex",
        className,
      )}
      style={style}
    >
      <div className="min-h-0 flex-1 overflow-y-auto p-2 scrollbar-thin">
        <div className="space-y-1">
          <button
            type="button"
            className={sessionButtonClass(false)}
            onClick={() => onSessionChange({ kind: "new" })}
          >
            <PlusCircle className="mt-0.5 h-4 w-4 shrink-0" />
            <span className="min-w-0 flex-1">
              <span className="block truncate text-foreground/90">
                {t("New chat")}
              </span>
            </span>
          </button>
        </div>

        <div className="mt-4">
          {agents.length > 0 && (
            <div className="mb-3">
              <div className="mb-2 px-2 font-mono text-[10px] font-semibold uppercase tracking-wide text-muted-foreground/60">
                {t("Filter")}
              </div>
              <div className="flex flex-wrap gap-1.5 px-2">
                <button
                  type="button"
                  className={cn(
                    "flex h-7 min-w-8 items-center justify-center rounded-md border px-2 font-mono text-[10px] font-semibold uppercase transition-colors",
                    selectedAgentFilter === ALL_AGENTS_FILTER
                      ? "border-primary/50 bg-primary/10 text-primary"
                      : "border-border/70 bg-background/70 text-muted-foreground hover:bg-muted/70 hover:text-foreground",
                  )}
                  title={t("All agents")}
                  aria-label={t("All agents")}
                  aria-pressed={selectedAgentFilter === ALL_AGENTS_FILTER}
                  onClick={() => onAgentFilterChange(ALL_AGENTS_FILTER)}
                >
                  {t("ALL")}
                </button>
                {agents.map((agent) => {
                  const selected = agent.id === selectedAgentFilter;
                  return (
                    <button
                      key={agent.id}
                      type="button"
                      className={cn(
                        "flex h-7 w-7 items-center justify-center rounded-md border transition-colors",
                        selected
                          ? "border-primary/50 bg-primary/10 text-primary"
                          : "border-border/70 bg-background/70 text-muted-foreground hover:bg-muted/70 hover:text-foreground",
                      )}
                      title={agent.name}
                      aria-label={agent.name}
                      aria-pressed={selected}
                      onClick={() => onAgentFilterChange(agent.id)}
                    >
                      <BrandIcon
                        kind="cli"
                        id={agent.id}
                        label={agent.name}
                        className="h-4 w-4"
                      />
                    </button>
                  );
                })}
              </div>
            </div>
          )}
          {sessionsLoading && workspaceGroups.length === 0 ? (
            <div className="flex items-center gap-2 px-2 py-4 text-xs text-muted-foreground">
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
              {t("Loading sessions...")}
            </div>
          ) : workspaceGroups.length === 0 ? (
            <div className="px-2 py-4 text-xs text-muted-foreground/60">
              {t("No projects")}
            </div>
          ) : (
            <div className="space-y-5">
              <div className="flex items-center justify-between gap-2 px-2">
                <div className="font-mono text-[10px] font-semibold uppercase tracking-wide text-muted-foreground/60">
                  {t("Projects")}
                </div>
                <button
                  type="button"
                  className="flex h-6 w-6 items-center justify-center rounded-md text-muted-foreground/60 transition hover:bg-muted/70 hover:text-foreground focus-visible:bg-muted/70 focus-visible:text-foreground focus-visible:outline-none"
                  title={t("Sync sessions")}
                  aria-label={t("Sync sessions")}
                  disabled={sessionsLoading}
                  onClick={onSyncSessions}
                >
                  {sessionsLoading ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  ) : (
                    <RefreshCw className="h-3.5 w-3.5" />
                  )}
                </button>
              </div>
              {workspaceGroups.map((group) => {
                const workspacePath = group.workspace.path;
                const collapsed = collapsedWorkspaces[workspacePath] ?? false;
                const sessionsExpanded = expandedSessionLists[workspacePath] ?? false;
                const sortedSessions = sortSessionsByUpdatedAt(group.sessions);
                const visibleSessions = sessionsExpanded
                  ? sortedSessions
                  : sortedSessions.slice(0, SESSION_PREVIEW_LIMIT);
                const hiddenSessionCount = Math.max(
                  sortedSessions.length - SESSION_PREVIEW_LIMIT,
                  0,
                );
                const workspaceName = workspaceLabel(workspacePath);
                return (
                  <section key={workspacePath}>
                    <button
                      type="button"
                      className="mb-1.5 flex w-full min-w-0 items-start gap-1.5 rounded-md px-1.5 py-1 text-left transition-colors hover:bg-muted/60"
                      title={workspacePath}
                      aria-expanded={!collapsed}
                      aria-label={
                        collapsed
                          ? t("Expand workspace {{workspace}}", { workspace: workspaceName })
                          : t("Collapse workspace {{workspace}}", { workspace: workspaceName })
                      }
                      onClick={() => toggleWorkspace(workspacePath)}
                    >
                      {collapsed ? (
                        <ChevronRight className="mt-0.5 h-3.5 w-3.5 shrink-0 text-muted-foreground/70" />
                      ) : (
                        <ChevronDown className="mt-0.5 h-3.5 w-3.5 shrink-0 text-muted-foreground/70" />
                      )}
                      <Folder className="mt-0.5 h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                      <span className="min-w-0 flex-1">
                        <span className="block truncate text-xs font-medium text-foreground/75">
                          {workspaceName}
                        </span>
                        <span className="block truncate text-[10px] leading-4 text-muted-foreground/45">
                          {workspacePath}
                        </span>
                      </span>
                    </button>
                    {!collapsed && (
                      <div className="space-y-1 pl-5">
                        {sortedSessions.length === 0 ? (
                          <div className="px-2 py-1 text-xs text-muted-foreground/45">
                            {t("No chats")}
                          </div>
                        ) : (
                          <>
                            {visibleSessions.map((session) => {
                              const active =
                                sessionSelection.kind === "resume" &&
                                activeAgentId === session.agent_id &&
                                sessionSelection.sessionId === session.session_id;
                              const loading =
                                loadingSessionId === session.session_id ||
                                loadingSessionKeys?.has(chatSessionKey(session));
                              const archiving = archivingSessionId === session.session_id;
                              const archived = session.archived;
                              const sessionHostAgentId =
                                session.host_agent_id ?? session.agent_id;
                              const sessionAgentLabel = agentLabel(sessionHostAgentId);
                              const profileTypeLabel = sessionProfileTypeLabel(
                                session,
                                t("Native"),
                              );
                              const updatedAtLabel = formatSessionUpdatedAt(session.updated_at);
                              return (
                                <div
                                  key={chatSessionKey(session)}
                                  className="group/session relative"
                                >
                                  <button
                                    type="button"
                                    className={cn(sessionButtonClass(active, archived), "pr-8")}
                                    aria-busy={loading || archiving}
                                    onClick={() =>
                                      onSessionChange(
                                        {
                                          kind: "resume",
                                          sessionId: session.session_id,
                                        },
                                        session,
                                      )
                                    }
                                  >
                                    <SessionHostLogo
                                      agentId={sessionHostAgentId}
                                      agentLabel={sessionAgentLabel}
                                      profileId={session.host_profile_id}
                                      profileLabel={session.host_profile_label}
                                      providerId={session.host_provider}
                                      providerLabel={session.host_provider_label}
                                      archived={archived}
                                      size="sm"
                                      className="mt-0.5"
                                    />
                                    <span className="min-w-0 flex-1">
                                      <span
                                        className={cn(
                                          "block truncate",
                                          archived
                                            ? "text-muted-foreground/55"
                                            : "text-foreground/90",
                                        )}
                                      >
                                        {session.title}
                                      </span>
                                      <span
                                        className={cn(
                                          "mt-0.5 flex min-w-0 items-center gap-1.5 text-[11px] leading-4",
                                          archived
                                            ? "text-muted-foreground/35"
                                            : "text-muted-foreground",
                                        )}
                                      >
                                        <span
                                          className={cn(
                                            "inline-flex max-w-[7rem] shrink-0 items-center rounded-full border px-1.5 py-0.5 text-[10px] font-medium leading-none",
                                            archived
                                              ? "border-border/40 bg-muted/20 text-muted-foreground/45"
                                              : "border-border/70 bg-background/80 text-muted-foreground",
                                          )}
                                          title={profileTypeLabel}
                                        >
                                          <span className="truncate">{profileTypeLabel}</span>
                                        </span>
                                        {updatedAtLabel && (
                                          <span className="min-w-0 truncate">
                                            {updatedAtLabel}
                                          </span>
                                        )}
                                      </span>
                                    </span>
                                    {loading && (
                                      <Loader2 className="mt-0.5 h-3.5 w-3.5 shrink-0 animate-spin text-primary" />
                                    )}
                                  </button>
                                  <button
                                    type="button"
                                    className={cn(
                                      "absolute right-1.5 top-1.5 flex h-6 w-6 items-center justify-center rounded-md text-muted-foreground/50 transition hover:bg-background/80 hover:text-foreground focus-visible:bg-background/80 focus-visible:text-foreground focus-visible:outline-none",
                                      variant === "desktop"
                                        ? "opacity-0 group-hover/session:opacity-100 group-focus-within/session:opacity-100"
                                        : "opacity-100",
                                    )}
                                    title={t("Archive chat")}
                                    aria-label={t("Archive chat")}
                                    disabled={archiving}
                                    onClick={(event) => {
                                      event.stopPropagation();
                                      onArchiveSession(session);
                                    }}
                                  >
                                    {archiving ? (
                                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                                    ) : (
                                      <Archive className="h-3.5 w-3.5" />
                                    )}
                                  </button>
                                </div>
                              );
                            })}
                            {hiddenSessionCount > 0 && (
                              <button
                                type="button"
                                className="w-full rounded-md px-2 py-1 text-left text-[11px] text-muted-foreground/50 transition-colors hover:bg-muted/40 hover:text-muted-foreground"
                                onClick={() => toggleSessionList(workspacePath)}
                              >
                                {sessionsExpanded
                                  ? t("Show less")
                                  : t("Show more {{count}}", { count: hiddenSessionCount })}
                              </button>
                            )}
                          </>
                        )}
                      </div>
                    )}
                  </section>
                );
              })}
            </div>
          )}
        </div>
      </div>
    </aside>
  );
}
