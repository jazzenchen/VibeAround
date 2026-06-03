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
    <div className="relative z-10 flex min-w-0 flex-1 items-center justify-center gap-2">
      {WIZARD_STEPS.map((step, index) => {
        const active = index === activeIndex;
        const done = index < activeIndex;
        return (
          <div key={step.id} className="flex items-center gap-2">
            <div
              className={cn(
                "flex h-7 items-center gap-2 rounded-full px-2.5 text-xs transition-colors",
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
              <span className={cn("hidden font-medium md:inline", active && "inline")}>
                {step.label}
              </span>
            </div>
            {index < WIZARD_STEPS.length - 1 && (
              <span className="h-px w-6 bg-border" aria-hidden="true" />
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
    <aside className="min-h-0 border-r border-border bg-muted/20 p-8">
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
          <div className="rounded-md border border-border bg-background/70 px-3 py-2 text-xs text-muted-foreground">
            {meta.hint}
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
  hint: string;
  icon: ReactNode;
} {
  switch (step) {
    case "agents":
      return {
        eyebrow: "Step 1",
        title: "Start with your coding agents.",
        body: "Auto setup reuses working tools and prepares only what is missing.",
        hint: "Recommended defaults are safe. You can change everything later.",
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
