export type WizardStepId = "agents" | "im" | "remote" | "install" | "configure";

export interface WizardStep {
  id: WizardStepId;
  label: string;
}

export const WIZARD_STEPS: WizardStep[] = [
  { id: "agents", label: "Coding agent" },
  { id: "im", label: "IM access" },
  { id: "remote", label: "Remote browser" },
  { id: "install", label: "Install" },
  { id: "configure", label: "Configure" },
];
