import {
  Bot,
  CheckCircle2,
  Globe,
  KeyRound,
  MessageSquare,
  Wrench,
} from "lucide-react";
import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

import { WIZARD_STEPS, type WizardStepId } from "../wizardTypes";

export function ProgressStepper({ activeIndex }: { activeIndex: number }) {
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
                {step.label}
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
          <div className="text-[11px] font-medium uppercase text-muted-foreground">
            {meta.eyebrow}
          </div>
          <h1 className="text-3xl font-semibold leading-tight">
            {meta.title}
          </h1>
          <p className="text-sm leading-6 text-muted-foreground">
            {meta.body}
          </p>
          {meta.hint && (
            <p className="text-xs leading-5 text-muted-foreground">
              {meta.hint}
            </p>
          )}
        </div>
      </div>
    </aside>
  );
}

function questionCopy(step: WizardStepId): {
  eyebrow: string;
  title: string;
  body: string;
  hint?: string;
  icon: ReactNode;
} {
  switch (step) {
    case "agents":
      return {
        eyebrow: "Step 1",
        title: "Start with your coding agents.",
        body: "Choose the coding agents VibeAround should prepare for this computer.",
        icon: <Bot className="h-5 w-5" />,
      };
    case "im":
      return {
        eyebrow: "Step 2",
        title: "Choose your IM entry points.",
        body: "Pick the apps you use. Login and tokens wait until the final step.",
        hint: "Skip this if you only plan to use the desktop app.",
        icon: <MessageSquare className="h-5 w-5" />,
      };
    case "remote":
      return {
        eyebrow: "Step 3",
        title: "Decide on remote access.",
        body: "Cloudflare gives this machine a stable public route when you need one.",
        hint: "Local-only setups can skip this step.",
        icon: <Globe className="h-5 w-5" />,
      };
    case "install":
      return {
        eyebrow: "Setup",
        title: "Let Startkit prepare the computer.",
        body: "The check runs automatically. Install only the selected pieces.",
        hint: "Details stay available, but the main flow stays simple.",
        icon: <Wrench className="h-5 w-5" />,
      };
    case "configure":
      return {
        eyebrow: "Final step",
        title: "Finish the parts that need you.",
        body: "Add API profiles, IM login, or tunnel tokens only when selected.",
        hint: "Empty sections are hidden automatically.",
        icon: <KeyRound className="h-5 w-5" />,
      };
  }
}
