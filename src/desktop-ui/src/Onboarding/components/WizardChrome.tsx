import {
  Bot,
  CheckCircle2,
  Globe,
  KeyRound,
  Loader2,
  MessageSquare,
  ShieldCheck,
  Wrench,
} from "lucide-react";
import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

import { WIZARD_STEPS, type WizardStepId } from "../wizardTypes";
import type {
  AgentSummary,
  PluginRegistryEntry,
  StartkitChoices,
  StartkitItemReport,
} from "../types";

export function ProgressStepper({ activeIndex }: { activeIndex: number }) {
  return (
    <div className="relative z-10 flex min-w-0 flex-1 items-center gap-1.5">
      {WIZARD_STEPS.map((step, index) => {
        const active = index === activeIndex;
        const done = index < activeIndex;
        return (
          <div key={step.id} className="flex min-w-0 flex-1 items-center gap-1.5">
            <div
              className={cn(
                "flex min-w-0 flex-1 items-center gap-2 rounded-md border px-2.5 py-1.5 text-xs transition-colors",
                active
                  ? "border-primary bg-primary/10 text-primary"
                  : done
                    ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300"
                    : "border-border bg-background text-muted-foreground",
              )}
            >
              <span
                className={cn(
                  "flex h-5 w-5 shrink-0 items-center justify-center rounded-full text-[10px]",
                  active
                    ? "bg-primary text-primary-foreground"
                    : done
                      ? "bg-emerald-500 text-white"
                      : "bg-muted text-muted-foreground",
                )}
              >
                {done ? <CheckCircle2 className="h-3 w-3" /> : index + 1}
              </span>
              <span className="truncate font-medium">{step.label}</span>
            </div>
          </div>
        );
      })}
    </div>
  );
}

export function QuestionPane({
  step,
  choices,
  agents,
  pluginRegistry,
  tunnelProvider,
  reports,
  scanning,
}: {
  step: WizardStepId;
  choices: StartkitChoices;
  agents: AgentSummary[];
  pluginRegistry: PluginRegistryEntry[];
  tunnelProvider: string;
  reports: StartkitItemReport[];
  scanning: boolean;
}) {
  const meta = questionCopy(step);
  const selectedAgents = agents.filter((agent) => choices.agents.includes(agent.id));
  const selectedPlugins = pluginRegistry.filter((plugin) =>
    choices.channels.includes(plugin.id),
  );
  const attentionCount = reports.filter((report) =>
    ["missing", "outdated", "broken", "blocked", "error"].includes(report.status),
  ).length;

  return (
    <aside className="min-h-0 overflow-y-auto border-r border-border bg-muted/20 p-6">
      <div
        key={step}
        className="flex min-h-full flex-col animate-in fade-in slide-in-from-left-1 duration-300"
      >
        <div className="mb-6 flex h-10 w-10 items-center justify-center rounded-md border border-primary/25 bg-primary/10 text-primary">
          {meta.icon}
        </div>
        <div className="space-y-3">
          <div className="text-[11px] font-medium uppercase text-muted-foreground">
            {meta.eyebrow}
          </div>
          <h1 className="max-w-sm text-2xl font-semibold leading-tight">
            {meta.title}
          </h1>
          <p className="max-w-sm text-sm leading-6 text-muted-foreground">
            {meta.body}
          </p>
        </div>

        <div className="mt-8 space-y-3">
          <SummaryLine
            icon={<Bot className="h-4 w-4" />}
            label="Agents"
            value={
              selectedAgents.length > 0
                ? selectedAgents.map((agent) => agent.display_name).join(", ")
                : "Skipped"
            }
          />
          <SummaryLine
            icon={<MessageSquare className="h-4 w-4" />}
            label="IM"
            value={
              selectedPlugins.length > 0
                ? selectedPlugins.map((plugin) => plugin.name).join(", ")
                : "Skipped"
            }
          />
          <SummaryLine
            icon={<Globe className="h-4 w-4" />}
            label="Remote"
            value={tunnelProvider === "none" ? "Skipped" : tunnelProvider}
          />
        </div>

        <div className="mt-auto pt-8">
          <div className="rounded-md border border-border bg-background p-3">
            <div className="flex items-center gap-2 text-xs font-medium">
              {scanning ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin text-primary" />
              ) : (
                <ShieldCheck className="h-3.5 w-3.5 text-primary" />
              )}
              Environment check
            </div>
            <p className="mt-1 text-[11px] leading-snug text-muted-foreground">
              {scanning
                ? "Checking the selected setup in the background."
                : attentionCount > 0
                  ? `${attentionCount} selected item${attentionCount === 1 ? "" : "s"} need setup.`
                  : "Selected items look ready or will be prepared later."}
            </p>
          </div>
        </div>
      </div>
    </aside>
  );
}

function questionCopy(step: WizardStepId): {
  eyebrow: string;
  title: string;
  body: string;
  icon: ReactNode;
} {
  switch (step) {
    case "agents":
      return {
        eyebrow: "Step 1",
        title: "Choose how VibeAround should find your coding agents.",
        body:
          "If you already have Claude or Codex on PATH, Startkit can reuse them. If not, it can install managed copies under VibeAround.",
        icon: <Bot className="h-5 w-5" />,
      };
    case "im":
      return {
        eyebrow: "Step 2",
        title: "Decide whether messages should reach your agents.",
        body:
          "Pick the IM tools you actually use. Plugins install later as part of the single setup run, and login can wait until the final configuration step.",
        icon: <MessageSquare className="h-5 w-5" />,
      };
    case "remote":
      return {
        eyebrow: "Step 3",
        title: "Choose whether this computer needs remote browser access.",
        body:
          "Cloudflare is the recommended route for a stable public address. Skipping is fine for local-only use, and this can be changed later.",
        icon: <Globe className="h-5 w-5" />,
      };
    case "install":
      return {
        eyebrow: "Setup",
        title: "Startkit installs only what your choices require.",
        body:
          "The scan runs automatically. Review the grouped plan, then let Startkit prepare the selected basics, agents, IM plugins, and remote tools.",
        icon: <Wrench className="h-5 w-5" />,
      };
    case "configure":
      return {
        eyebrow: "Final step",
        title: "Add the secrets and logins that need your hand.",
        body:
          "API profiles, QR login, and tunnel tokens are saved here after the binaries and plugins are ready.",
        icon: <KeyRound className="h-5 w-5" />,
      };
  }
}

function SummaryLine({
  icon,
  label,
  value,
}: {
  icon: ReactNode;
  label: string;
  value: string;
}) {
  return (
    <div className="flex items-start gap-2 rounded-md border border-border bg-background px-3 py-2">
      <span className="mt-0.5 text-primary">{icon}</span>
      <div className="min-w-0">
        <div className="text-[11px] font-medium text-muted-foreground">
          {label}
        </div>
        <div className="truncate text-xs font-medium">{value}</div>
      </div>
    </div>
  );
}
