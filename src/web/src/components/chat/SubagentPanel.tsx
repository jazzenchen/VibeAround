"use client";

import { useEffect, useMemo, type ComponentType, type ReactNode } from "react";
import type { MultiAgentTurn, ThreadAgent } from "@va/client";
import {
  AlertCircle,
  Bot,
  CheckCircle2,
  CircleDot,
  FolderGit2,
  GitBranch,
  Loader2,
  PanelRightClose,
  PanelRightOpen,
} from "lucide-react";
import { useI18n } from "@va/i18n";
import { BrandIcon } from "@/components/brand-icon";
import { Button } from "@/components/ui/button";
import { getAgentDisplayName } from "@/lib/agents";
import { cn } from "@/lib/utils";

interface SubagentPanelProps {
  turns: MultiAgentTurn[];
  agents: ThreadAgent[];
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
  open,
  selectedAgentId,
  onOpenChange,
  onSelectedAgentChange,
}: SubagentPanelProps) {
  const { t } = useI18n();
  const sortedAgents = useMemo(
    () =>
      [...agents].sort((a, b) =>
        a.created_at === b.created_at ? a.name.localeCompare(b.name) : a.created_at.localeCompare(b.created_at),
      ),
    [agents],
  );
  const selectedAgent =
    sortedAgents.find((agent) => agent.id === selectedAgentId) ?? sortedAgents[0];
  const selectedTurn = selectedAgent
    ? turns.find((turn) => turn.id === selectedAgent.turn_id)
    : turns[0];

  useEffect(() => {
    if (selectedAgent && selectedAgent.id !== selectedAgentId) {
      onSelectedAgentChange(selectedAgent.id);
    }
  }, [onSelectedAgentChange, selectedAgent, selectedAgentId]);

  if (sortedAgents.length === 0) return null;

  if (!open) {
    return (
      <aside className="hidden h-full w-11 shrink-0 border-l border-border/60 bg-background lg:flex">
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
    <aside className="hidden h-full w-[22rem] shrink-0 flex-col border-l border-border/60 bg-background lg:flex">
      <div className="flex h-12 shrink-0 items-center justify-between gap-2 border-b border-border/60 px-3">
        <div className="min-w-0">
          <div className="truncate text-sm font-medium text-foreground">
            {t("Subagents")}
          </div>
          <div className="truncate text-[11px] text-muted-foreground">
            {selectedTurn
              ? t("{{mode}} turn", { mode: selectedTurn.mode })
              : t("Multi-agent turn")}
          </div>
        </div>
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          className="shrink-0 text-muted-foreground hover:text-foreground"
          onClick={() => onOpenChange(false)}
          title={t("Hide subagents")}
          aria-label={t("Hide subagents")}
        >
          <PanelRightClose className="h-4 w-4" />
        </Button>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="space-y-1.5 border-b border-border/60 p-2">
          {sortedAgents.map((agent) => {
            const active = selectedAgent?.id === agent.id;
            const meta = statusMeta[agent.status];
            const StatusIcon = meta.icon;
            return (
              <button
                key={agent.id}
                type="button"
                className={cn(
                  "flex min-h-12 w-full items-center gap-2 rounded-md px-2 py-1.5 text-left transition-colors",
                  active
                    ? "bg-primary/10 text-foreground"
                    : "text-muted-foreground hover:bg-muted/70 hover:text-foreground",
                )}
                onClick={() => onSelectedAgentChange(agent.id)}
              >
                <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground">
                  <BrandIcon
                    kind="cli"
                    id={agent.agent_id}
                    label={getAgentDisplayName(agent.agent_id)}
                    className="h-4 w-4"
                  />
                </div>
                <div className="min-w-0 flex-1">
                  <div className="truncate text-sm font-medium">{agent.name}</div>
                  <div className="truncate text-[11px]">
                    {getAgentDisplayName(agent.agent_id)}
                  </div>
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
          })}
        </div>

        {selectedAgent && (
          <div className="space-y-4 p-3">
            <div className="space-y-2">
              <div className="flex items-center gap-2">
                <Bot className="h-4 w-4 text-muted-foreground" />
                <div className="min-w-0">
                  <div className="truncate text-sm font-medium text-foreground">
                    {selectedAgent.name}
                  </div>
                  <div className="truncate text-[11px] text-muted-foreground">
                    {selectedAgent.id}
                  </div>
                </div>
              </div>
              <StatusBadge status={selectedAgent.status} />
            </div>

            {selectedAgent.task && (
              <PanelSection title={t("Task")}>
                <p className="whitespace-pre-wrap break-words text-sm leading-5 text-foreground">
                  {selectedAgent.task}
                </p>
              </PanelSection>
            )}

            <PanelSection title={t("Workspace")}>
              <PathRow icon={GitBranch} value={selectedAgent.branch} />
              <PathRow icon={FolderGit2} value={selectedAgent.worktree} />
            </PanelSection>

            <PanelSection title={t("Messages")}>
              <div className="rounded-md border border-dashed border-border/70 px-3 py-6 text-center text-xs text-muted-foreground">
                {t("Waiting for report")}
              </div>
            </PanelSection>
          </div>
        )}
      </div>
    </aside>
  );
}

function StatusBadge({ status }: { status: ThreadAgent["status"] }) {
  const { t } = useI18n();
  const meta = statusMeta[status];
  const Icon = meta.icon;
  return (
    <div className={cn("inline-flex items-center gap-1.5 text-xs", meta.className)}>
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
