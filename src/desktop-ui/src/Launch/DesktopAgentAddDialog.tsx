import { Monitor } from "lucide-react";
import { useI18n } from "@va/i18n";

import { BrandIcon } from "@/components/brand-icon";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { AgentSummary } from "./api";

interface Props {
  open: boolean;
  agents: AgentSummary[];
  onOpenChange: (open: boolean) => void;
  onSelect: (agent: AgentSummary) => void;
}

export function DesktopAgentAddDialog({
  open,
  agents,
  onOpenChange,
  onSelect,
}: Props) {
  const { t } = useI18n();

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{t("Add desktop agent")}</DialogTitle>
          <DialogDescription>
            {t("Choose a desktop agent to add manually.")}
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-2">
          {agents.map((agent) => (
            <button
              key={agent.id}
              type="button"
              className="flex items-center gap-3 rounded-md border border-border bg-card px-3 py-2.5 text-left transition-colors hover:border-primary/40 hover:bg-accent/35"
              onClick={() => onSelect(agent)}
            >
              <BrandIcon
                kind="cli"
                id={agent.id}
                label={agent.display_name}
                framed={false}
                className="h-10 w-10 rounded-lg"
              />
              <span className="min-w-0 flex-1">
                <span className="block text-sm font-medium">
                  {agent.display_name}
                </span>
                <span className="block truncate text-xs text-muted-foreground">
                  {agent.description}
                </span>
              </span>
              <Monitor className="h-4 w-4 shrink-0 text-muted-foreground" />
            </button>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  );
}
