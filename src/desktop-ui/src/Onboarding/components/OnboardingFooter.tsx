import type { ReactNode } from "react";
import { ArrowLeft } from "lucide-react";
import { useI18n } from "@va/i18n";

import { Button } from "@/components/ui/button";

import type { WizardStepId } from "../wizardTypes";

export interface PrimaryAction {
  label: string;
  icon: ReactNode;
  disabled: boolean;
  run: () => void;
}

export function OnboardingFooter({
  activeStep,
  activeIndex,
  running,
  finishing,
  primaryAction,
  onBack,
  onSkip,
  onCancel,
}: {
  activeStep: WizardStepId;
  activeIndex: number;
  running: boolean;
  finishing: boolean;
  primaryAction: PrimaryAction;
  onBack: () => void;
  onSkip: () => void;
  onCancel: () => void;
}) {
  const { t } = useI18n();
  const canSkip =
    activeStep === "agents" ||
    activeStep === "im" ||
    activeStep === "remote";
  const footerHint =
    activeStep === "agents"
      ? t(
          "Keep the defaults if you are not sure; everything can be changed later.",
        )
      : null;

  return (
    <footer className="relative flex h-14 items-center gap-3 border-t border-border px-5">
      <div className="flex items-center gap-2">
        <Button
          type="button"
          variant="outline"
          onClick={onBack}
          disabled={activeIndex === 0 || running || finishing}
        >
          <ArrowLeft className="h-4 w-4" />
          {t("Back")}
        </Button>
      </div>
      {footerHint && (
        <div className="pointer-events-none absolute left-1/2 max-w-md -translate-x-1/2 px-4 text-center text-xs text-muted-foreground">
          {footerHint}
        </div>
      )}
      <div className="ml-auto flex items-center gap-2">
        {running && activeStep === "install" && (
          <Button type="button" variant="outline" onClick={onCancel}>
            {t("Cancel")}
          </Button>
        )}
        {canSkip && (
          <Button
            type="button"
            variant="outline"
            onClick={onSkip}
            disabled={running || finishing}
          >
            {t("Skip")}
          </Button>
        )}
        <Button
          type="button"
          onClick={primaryAction.run}
          disabled={primaryAction.disabled}
        >
          {primaryAction.icon}
          {primaryAction.label}
        </Button>
      </div>
    </footer>
  );
}
