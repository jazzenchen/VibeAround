"use client";

import {
  useEffect,
  useMemo,
  useState,
  type ComponentType,
  type ReactNode,
} from "react";
import type { MultiAgentTurn, ThreadAgent } from "@va/client";
import {
  AlertCircle,
  CheckCircle2,
  CircleDot,
  Columns3,
  FolderGit2,
  GitBranch,
  Loader2,
  Maximize2,
  Minimize2,
  PanelRightClose,
  PanelRightOpen,
  X,
} from "lucide-react";
import { useI18n } from "@va/i18n";
import { BrandIcon } from "@/components/brand-icon";
import { Button } from "@/components/ui/button";
import { getAgentDisplayName } from "@/lib/agents";
import { cn } from "@/lib/utils";
import { ChatMessageList } from "./ChatMessageList";
import type { ChatDisplaySettings, ChatMessage } from "./chatTypes";

interface SubagentPanelProps {
  turns: MultiAgentTurn[];
  agents: ThreadAgent[];
  messagesByAgent: Record<string, ChatMessage[]>;
  displaySettings: ChatDisplaySettings;
  open: boolean;
  selectedAgentId?: string;
  onOpenChange: (open: boolean) => void;
  onSelectedAgentChange: (agentId: string) => void;
}

const statusMeta = {
  ready: {
    label: "Ready",
    className: "text-muted-foreground",
    icon: CircleDot,
  },
  running: {
    label: "Running",
    className: "text-primary",
    icon: Loader2,
  },
  completed: {
    label: "Completed",
    className: "text-emerald-600 dark:text-emerald-400",
    icon: CheckCircle2,
  },
  error: {
    label: "Error",
    className: "text-destructive",
    icon: AlertCircle,
  },
} as const;

export function SubagentPanel({
  turns,
  agents,
  messagesByAgent,
  displaySettings,
  open,
  selectedAgentId,
  onOpenChange,
  onSelectedAgentChange,
}: SubagentPanelProps) {
  const { t } = useI18n();
  const [expanded, setExpanded] = useState(false);
  const sortedAgents = useMemo(
    () =>
      [...agents].sort((a, b) =>
        a.created_at === b.created_at
          ? a.name.localeCompare(b.name)
          : a.created_at.localeCompare(b.created_at),
      ),
    [agents],
  );
  const selectedAgent =
    sortedAgents.find((agent) => agent.id === selectedAgentId) ?? sortedAgents[0];
  const activeTurn = selectedAgent
    ? turns.find((turn) => turn.id === selectedAgent.turn_id)
    : turns[0];

  useEffect(() => {
    if (selectedAgent && selectedAgent.id !== selectedAgentId) {
      onSelectedAgentChange(selectedAgent.id);
    }
  }, [onSelectedAgentChange, selectedAgent, selectedAgentId]);

  useEffect(() => {
    if (sortedAgents.length === 0 && expanded) {
      setExpanded(false);
    }
  }, [expanded, sortedAgents.length]);

  if (sortedAgents.length === 0) return null;

  if (!open) {
    return (
      <aside className="hidden h-full w-11 shrink-0 border-l border-border/60 bg-background md:flex">
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          className="m-1.5 text-muted-foreground hover:text-foreground"
          onClick={() => onOpenChange(true)}
          title={t("Show subagents")}
          aria-label={t("Show subagents")}
        >
          <PanelRightOpen className="h-4 w-4" />
        </Button>
      </aside>
    );
  }

  return (
    <>
      <aside className="hidden h-full w-[18.5rem] shrink-0 items-start justify-center border-l border-border/60 bg-background/60 px-3 py-4 md:flex">
        <section className="w-full rounded-lg border border-border/70 bg-background shadow-sm">
          <div className="flex items-center justify-between gap-2 px-4 py-3">
            <button
              type="button"
              className="flex min-w-0 items-center gap-1.5 text-left text-sm font-medium text-muted-foreground hover:text-foreground"
              onClick={() => setExpanded(true)}
            >
              <span className="truncate">{t("Progress")}</span>
              <Maximize2 className="h-3.5 w-3.5 shrink-0" />
            </button>
            <div className="flex shrink-0 items-center gap-1">
              <Button
                type="button"
                variant="ghost"
                size="icon-sm"
                className="h-7 w-7 text-muted-foreground hover:text-foreground"
                onClick={() => setExpanded(true)}
                title={t("Expand subagents")}
                aria-label={t("Expand subagents")}
              >
                <Columns3 className="h-4 w-4" />
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="icon-sm"
                className="h-7 w-7 text-muted-foreground hover:text-foreground"
                onClick={() => onOpenChange(false)}
                title={t("Hide subagents")}
                aria-label={t("Hide subagents")}
              >
                <PanelRightClose className="h-4 w-4" />
              </Button>
            </div>
          </div>

          <div className="border-t border-border/60 px-3 py-2">
            <div className="mb-2 flex items-center justify-between gap-2 text-[11px] text-muted-foreground">
              <span className="truncate">
                {activeTurn
                  ? t("{{mode}} turn", { mode: activeTurn.mode })
                  : t("Multi-agent turn")}
              </span>
              <span>{sortedAgents.length}</span>
            </div>
            <div className="space-y-1.5">
              {sortedAgents.map((agent) => (
                <SubagentListButton
                  key={agent.id}
                  agent={agent}
                  active={selectedAgent?.id === agent.id}
                  onClick={() => onSelectedAgentChange(agent.id)}
                />
              ))}
            </div>
          </div>
        </section>
      </aside>

      {expanded && (
        <SubagentExpandedView
          turns={turns}
          agents={sortedAgents}
          messagesByAgent={messagesByAgent}
          displaySettings={displaySettings}
          activeTurn={activeTurn}
          selectedAgentId={selectedAgent?.id}
          onSelectedAgentChange={onSelectedAgentChange}
          onClose={() => setExpanded(false)}
        />
      )}
    </>
  );
}

function SubagentListButton({
  agent,
  active,
  onClick,
}: {
  agent: ThreadAgent;
  active: boolean;
  onClick: () => void;
}) {
  const meta = statusMeta[agent.status];
  const StatusIcon = meta.icon;
  return (
    <button
      type="button"
      className={cn(
        "flex min-h-10 w-full items-center gap-2 rounded-md px-2 py-1.5 text-left transition-colors",
        active
          ? "bg-primary/10 text-foreground"
          : "text-muted-foreground hover:bg-muted/70 hover:text-foreground",
      )}
      onClick={onClick}
    >
      <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground">
        <BrandIcon
          kind="cli"
          id={agent.agent_id}
          label={getAgentDisplayName(agent.agent_id)}
          className="h-3.5 w-3.5"
        />
      </div>
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm font-medium">{agent.name}</div>
        <div className="truncate text-[11px]">{getAgentDisplayName(agent.agent_id)}</div>
      </div>
      <StatusIcon
        className={cn(
          "h-3.5 w-3.5 shrink-0",
          meta.className,
          agent.status === "running" && "animate-spin",
        )}
      />
    </button>
  );
}

function SubagentExpandedView({
  turns,
  agents,
  messagesByAgent,
  displaySettings,
  activeTurn,
  selectedAgentId,
  onSelectedAgentChange,
  onClose,
}: {
  turns: MultiAgentTurn[];
  agents: ThreadAgent[];
  messagesByAgent: Record<string, ChatMessage[]>;
  displaySettings: ChatDisplaySettings;
  activeTurn?: MultiAgentTurn;
  selectedAgentId?: string;
  onSelectedAgentChange: (agentId: string) => void;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const turn = activeTurn ?? turns[0];
  const turnAgents = turn
    ? agents.filter((agent) => agent.turn_id === turn.id)
    : agents;
  const columns = turnAgents.length > 0 ? turnAgents : agents;

  return (
    <div className="fixed inset-0 z-50 flex flex-col bg-background">
      <header className="flex h-14 shrink-0 items-center justify-between gap-3 border-b border-border/60 px-4">
        <div className="flex min-w-0 items-center gap-2">
          <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground">
            <Columns3 className="h-4 w-4" />
          </div>
          <div className="min-w-0">
            <div className="truncate text-sm font-medium text-foreground">
              {turn ? t("{{mode}} subagents", { mode: turn.mode }) : t("Subagents")}
            </div>
            <div className="truncate text-[11px] text-muted-foreground">
              {turn?.id ?? t("Current thread")}
            </div>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            className="text-muted-foreground hover:text-foreground"
            onClick={onClose}
            title={t("Collapse")}
            aria-label={t("Collapse")}
          >
            <Minimize2 className="h-4 w-4" />
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            className="text-muted-foreground hover:text-foreground"
            onClick={onClose}
            title={t("Close")}
            aria-label={t("Close")}
          >
            <X className="h-4 w-4" />
          </Button>
        </div>
      </header>

      <main className="min-h-0 flex-1 overflow-auto p-4">
        {turn?.mode === "parallel" || !turn ? (
          <div
            className="grid min-h-full gap-3"
            style={{
              gridTemplateColumns: `repeat(${Math.max(columns.length, 1)}, minmax(19rem, 1fr))`,
            }}
          >
            {columns.map((agent) => (
              <ParallelAgentColumn
                key={agent.id}
                agent={agent}
                messages={messagesByAgent[agent.id] ?? []}
                displaySettings={displaySettings}
                active={selectedAgentId === agent.id}
                onSelect={() => onSelectedAgentChange(agent.id)}
              />
            ))}
          </div>
        ) : (
          <div className="mx-auto grid max-w-5xl gap-3">
            {columns.map((agent) => (
              <ParallelAgentColumn
                key={agent.id}
                agent={agent}
                messages={messagesByAgent[agent.id] ?? []}
                displaySettings={displaySettings}
                active={selectedAgentId === agent.id}
                onSelect={() => onSelectedAgentChange(agent.id)}
              />
            ))}
          </div>
        )}
      </main>
    </div>
  );
}

function ParallelAgentColumn({
  agent,
  messages,
  displaySettings,
  active,
  onSelect,
}: {
  agent: ThreadAgent;
  messages: ChatMessage[];
  displaySettings: ChatDisplaySettings;
  active: boolean;
  onSelect: () => void;
}) {
  const { t } = useI18n();
  const report = parseSubagentReport(agent.report);
  return (
    <section
      className={cn(
        "flex min-h-[calc(100vh-6.5rem)] min-w-0 flex-col rounded-lg border bg-background",
        active ? "border-primary/40 shadow-sm" : "border-border/70",
      )}
    >
      <button
        type="button"
        className="flex min-h-16 items-center gap-2 border-b border-border/60 px-3 py-2 text-left"
        onClick={onSelect}
      >
        <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground">
          <BrandIcon
            kind="cli"
            id={agent.agent_id}
            label={getAgentDisplayName(agent.agent_id)}
            className="h-4 w-4"
          />
        </div>
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-medium text-foreground">
            {agent.name}
          </div>
          <div className="truncate text-[11px] text-muted-foreground">
            {getAgentDisplayName(agent.agent_id)}
          </div>
        </div>
        <StatusBadge status={agent.status} />
      </button>

      <div className="min-h-0 flex-1 space-y-4 overflow-y-auto p-3">
        {agent.task && (
          <PanelSection title={t("Context")}>
            <p className="whitespace-pre-wrap break-words text-sm leading-5 text-foreground">
              {agent.task}
            </p>
          </PanelSection>
        )}

        <PanelSection title={t("Workspace")}>
          <PathRow icon={GitBranch} value={agent.branch} />
          <PathRow icon={FolderGit2} value={agent.worktree} />
        </PanelSection>

        {agent.last_error && (
          <PanelSection title={t("Error")}>
            <p className="whitespace-pre-wrap break-words text-sm leading-5 text-destructive">
              {agent.last_error}
            </p>
          </PanelSection>
        )}

        {report && (
          <PanelSection title={t("Report")}>
            <SubagentReportSummary report={report} />
          </PanelSection>
        )}

        <PanelSection title={t("Messages")}>
          {messages.length === 0 ? (
            <div className="rounded-md border border-dashed border-border/70 px-3 py-8 text-center text-xs text-muted-foreground">
              {t("Waiting for report")}
            </div>
          ) : (
            <div className="flex h-[34rem] min-h-[22rem] overflow-hidden rounded-md border border-border/70 bg-background">
              <ChatMessageList
                messages={messages}
                streaming={agent.status === "running"}
                agentLabel={agent.name}
                displaySettings={displaySettings}
                workspacePath={agent.worktree}
              />
            </div>
          )}
        </PanelSection>
      </div>
    </section>
  );
}

interface ParsedSubagentReport {
  summary?: string;
  filesChanged: string[];
  tests: string[];
  notes: string[];
}

function parseSubagentReport(value: unknown): ParsedSubagentReport | undefined {
  if (!value || typeof value !== "object") return undefined;
  const record = value as Record<string, unknown>;
  return {
    summary:
      typeof record.summary === "string" && record.summary.trim()
        ? record.summary.trim()
        : undefined,
    filesChanged: stringList(record.files_changed),
    tests: stringList(record.tests),
    notes: stringList(record.notes),
  };
}

function stringList(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is string => typeof item === "string" && item.trim().length > 0);
}

function SubagentReportSummary({ report }: { report: ParsedSubagentReport }) {
  const { t } = useI18n();
  return (
    <div className="space-y-2 rounded-md border border-border/70 bg-muted/30 px-3 py-2">
      {report.summary && (
        <p className="whitespace-pre-wrap break-words text-sm leading-5 text-foreground">
          {report.summary}
        </p>
      )}
      <ReportList title={t("Files")} items={report.filesChanged} />
      <ReportList title={t("Tests")} items={report.tests} />
      <ReportList title={t("Notes")} items={report.notes} />
    </div>
  );
}

function ReportList({ title, items }: { title: string; items: string[] }) {
  if (items.length === 0) return null;
  return (
    <div className="space-y-1">
      <div className="text-[11px] font-medium text-muted-foreground">{title}</div>
      <ul className="space-y-1">
        {items.map((item, index) => (
          <li
            key={`${title}-${index}`}
            className="break-words rounded bg-background/70 px-2 py-1 text-xs leading-4 text-muted-foreground"
          >
            {item}
          </li>
        ))}
      </ul>
    </div>
  );
}

function StatusBadge({ status }: { status: ThreadAgent["status"] }) {
  const { t } = useI18n();
  const meta = statusMeta[status];
  const Icon = meta.icon;
  return (
    <div
      className={cn(
        "inline-flex shrink-0 items-center gap-1.5 text-xs",
        meta.className,
      )}
    >
      <Icon className={cn("h-3.5 w-3.5", status === "running" && "animate-spin")} />
      <span>{t(meta.label)}</span>
    </div>
  );
}

function PanelSection({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  return (
    <section className="space-y-2">
      <h3 className="text-[11px] font-medium uppercase text-muted-foreground">
        {title}
      </h3>
      {children}
    </section>
  );
}

function PathRow({
  icon: Icon,
  value,
}: {
  icon: ComponentType<{ className?: string }>;
  value: string;
}) {
  return (
    <div className="flex items-start gap-2 rounded-md bg-muted/60 px-2 py-2">
      <Icon className="mt-0.5 h-3.5 w-3.5 shrink-0 text-muted-foreground" />
      <code className="min-w-0 break-all text-[11px] leading-4 text-muted-foreground">
        {value}
      </code>
    </div>
  );
}
