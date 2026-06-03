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

  return (
    <footer className="flex h-14 items-center justify-between gap-3 border-t border-border px-5">
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
        {canSkip && (
          <Button
            type="button"
            variant="ghost"
            onClick={onSkip}
            disabled={running || finishing}
          >
            {t("Skip this step")}
          </Button>
        )}
      </div>
      <div className="flex items-center gap-2">
        {running && activeStep === "install" && (
          <Button type="button" variant="outline" onClick={onCancel}>
            {t("Cancel")}
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
