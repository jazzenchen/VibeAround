import {
  Bot,
  CheckCircle2,
  Globe,
  KeyRound,
  MessageSquare,
  Wrench,
} from "lucide-react";
import { useI18n } from "@va/i18n";
import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

import { WIZARD_STEPS, type WizardStepId } from "../wizardTypes";

export function ProgressStepper({ activeIndex }: { activeIndex: number }) {
  const { t } = useI18n();
  return (
    <div className="flex w-max items-center justify-center gap-1.5">
      {WIZARD_STEPS.map((step, index) => {
        const active = index === activeIndex;
        const done = index < activeIndex;
        return (
          <div key={step.id} className="flex min-w-0 items-center gap-1.5">
            <div
              className={cn(
                "flex h-7 min-w-0 items-center gap-1.5 rounded-full px-2 text-xs transition-colors",
                active
                  ? "bg-primary/10 text-primary"
                  : done
                    ? "text-emerald-700 dark:text-emerald-300"
                    : "text-muted-foreground",
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
              <span
                className={cn(
                  "hidden whitespace-nowrap font-medium lg:inline",
                  active && "inline",
                )}
              >
                {t(step.label)}
              </span>
            </div>
            {index < WIZARD_STEPS.length - 1 && (
              <span className="h-px w-4 bg-border" aria-hidden="true" />
            )}
          </div>
        );
      })}
    </div>
  );
}

export function QuestionPane({
  step,
}: {
  step: WizardStepId;
}) {
  const { t } = useI18n();
  const meta = questionCopy(step);

  return (
    <aside className="min-h-0 border-r border-border bg-muted/20 p-7">
      <div
        key={step}
        className="flex min-h-full flex-col justify-center animate-in fade-in slide-in-from-left-1 duration-300"
      >
        <div className="max-w-md space-y-5">
          <div className="flex h-12 w-12 items-center justify-center rounded-md border border-primary/25 bg-primary/10 text-primary">
            {meta.icon}
          </div>
          <h1 className="text-3xl font-semibold leading-tight">
            {t(meta.title)}
          </h1>
          {meta.body && (
            <p className="text-sm leading-6 text-muted-foreground">
              {t(meta.body)}
            </p>
          )}
          {meta.hint && (
            <p className="text-xs leading-5 text-muted-foreground">
              {t(meta.hint)}
            </p>
          )}
        </div>
      </div>
    </aside>
  );
}

function questionCopy(step: WizardStepId): {
  title: string;
  body?: string;
  hint?: string;
  icon: ReactNode;
} {
  switch (step) {
    case "agents":
      return {
        title: "Start with your coding agents.",
        body: "Claude Code and Codex CLI are recommended for daily vibe coding and vibe coding jobs.",
        icon: <Bot className="h-5 w-5" />,
      };
    case "im":
      return {
        title: "Choose your messaging apps.",
        hint: "Skip if you only use the coding agents on desktop.",
        icon: <MessageSquare className="h-5 w-5" />,
      };
    case "remote":
      return {
        title: "Configure remote access.",
        body: "Skip for local-only use.",
        icon: <Globe className="h-5 w-5" />,
      };
    case "install":
      return {
        title: "Install Components.",
        body: "Only missing items will be installed.",
        icon: <Wrench className="h-5 w-5" />,
      };
    case "configure":
      return {
        title: "Complete the configuration.",
        body: "Fill in API keys and other required details for your selected options.",
        icon: <KeyRound className="h-5 w-5" />,
      };
  }
}
