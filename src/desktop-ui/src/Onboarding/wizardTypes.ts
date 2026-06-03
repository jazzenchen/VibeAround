export type WizardStepId = "agents" | "im" | "remote" | "install" | "configure";

export interface WizardStep {
  id: WizardStepId;
  label: string;
}

export const WIZARD_STEPS: WizardStep[] = [
  { id: "agents", label: "Agents" },
  { id: "im", label: "IM" },
  { id: "remote", label: "Remote" },
  { id: "install", label: "Install" },
  { id: "configure", label: "Config" },
];
