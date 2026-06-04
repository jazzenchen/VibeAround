import {
  Bot,
  ChevronDown,
} from "lucide-react";
import { useMemo, useState } from "react";

import { BrandIcon } from "@/components/brand-icon";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { cn } from "@/lib/utils";

import { compactReportLabel } from "./startkitPresentation";
import type {
  AgentSummary,
  StartkitItemReport,
} from "../types";
import type { AgentId } from "../constants";

export function AgentDecisionPanel({
  agents,
  enabledAgents,
  reports,
  scanning,
  onToggleAgent,
}: {
  agents: AgentSummary[];
  enabledAgents: Set<AgentId>;
  reports: Map<string, StartkitItemReport>;
  scanning: boolean;
  onToggleAgent: (id: AgentId) => void;
}) {
  const [showMoreAgents, setShowMoreAgents] = useState(false);

  const recommendedAgents = useMemo(
    () => agents.filter((agent) => agent.id === "claude" || agent.id === "codex"),
    [agents],
  );
  const otherAgents = useMemo(
    () => agents.filter((agent) => agent.id !== "claude" && agent.id !== "codex"),
    [agents],
  );

  return (
    <div className="mx-auto flex min-h-full w-full max-w-4xl items-center py-4">
      <div className="w-full space-y-4">
        <section className="space-y-3">
          <div className="flex items-start justify-between gap-3 px-1">
            <div className="min-w-0">
              <div className="flex items-center gap-2 text-base font-semibold">
                <Bot className="h-4 w-4 text-primary" />
                Agents to enable
              </div>
              <p className="mt-1 text-sm text-muted-foreground">
                Select your favorite agents.
              </p>
            </div>
          </div>

          <AgentGrid
            agents={recommendedAgents}
            enabled={enabledAgents}
            reports={reports}
            scanning={scanning}
            onToggle={onToggleAgent}
          />

          {otherAgents.length > 0 && (
            <div className="flex items-center gap-3">
              <span className="h-px flex-1 bg-border" aria-hidden="true" />
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-7 shrink-0 px-1 text-xs text-muted-foreground hover:bg-transparent"
                onClick={() => setShowMoreAgents((value) => !value)}
              >
                <ChevronDown
                  className={cn(
                    "h-3.5 w-3.5 transition-transform",
                    showMoreAgents && "rotate-180",
                  )}
                />
                {showMoreAgents ? "Hide more agents" : "More agents"}
              </Button>
              <span className="h-px flex-1 bg-border" aria-hidden="true" />
            </div>
          )}

          {otherAgents.length > 0 && showMoreAgents && (
            <div className="animate-in fade-in slide-in-from-top-1 duration-200">
              <AgentGrid
                agents={otherAgents}
                enabled={enabledAgents}
                reports={reports}
                scanning={scanning}
                onToggle={onToggleAgent}
              />
            </div>
          )}
        </section>
      </div>
    </div>
  );
}

function AgentGrid({
  agents,
  enabled,
  reports,
  scanning,
  onToggle,
}: {
  agents: AgentSummary[];
  enabled: Set<string>;
  reports: Map<string, StartkitItemReport>;
  scanning: boolean;
  onToggle: (id: string) => void;
}) {
  return (
    <div className="grid gap-2 sm:grid-cols-2">
      {agents.map((agent) => {
        const selected = enabled.has(agent.id);
        const report = reports.get(`agents.${agent.id}.cli`);
        return (
          <button
            key={agent.id}
            type="button"
            className={cn(
              "relative flex min-h-[58px] items-center gap-3 rounded-md border p-2.5 pr-9 text-left transition-colors",
              selected
                ? "border-primary/50 bg-primary/10"
                : "border-border bg-background hover:border-primary/30",
            )}
            onClick={() => onToggle(agent.id)}
          >
            <BrandIcon
              kind="cli"
              id={agent.id}
              label={agent.display_name}
              className="h-7 w-7"
            />
            <span className="min-w-0 flex-1">
              <span className="block truncate text-sm font-medium">
                {agent.display_name}
              </span>
              <span className="mt-0.5 block truncate text-[11px] text-muted-foreground">
                {report ? compactReportLabel(report) : scanning ? "Checking" : "Not installed"}
              </span>
            </span>
            <Checkbox
              checked={selected}
              aria-hidden="true"
              tabIndex={-1}
              className="pointer-events-none absolute right-3 top-1/2 -translate-y-1/2"
            />
          </button>
        );
      })}
    </div>
  );
}
