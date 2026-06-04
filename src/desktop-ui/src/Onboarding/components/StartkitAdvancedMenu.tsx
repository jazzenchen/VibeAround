import {
  Globe,
  SlidersHorizontal,
  TerminalSquare,
} from "lucide-react";
import { useI18n } from "@va/i18n";

import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";

import type { StartkitManifestSummary } from "../types";

export function StartkitAdvancedMenu({
  sources,
  downloadSource,
  onDownloadSource,
  shellPath,
  shellPathDisabled,
  onShellPath,
}: {
  sources: StartkitManifestSummary["sources"];
  downloadSource: string;
  onDownloadSource: (value: string) => void;
  shellPath: boolean;
  shellPathDisabled: boolean;
  onShellPath: (checked: boolean) => void;
}) {
  const { t } = useI18n();
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          title={t("Settings")}
          aria-label={t("Settings")}
        >
          <SlidersHorizontal className="size-4 text-muted-foreground" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-80 p-3">
        <div className="space-y-4">
          <SourceChooser
            sources={sources}
            value={downloadSource}
            onChange={onDownloadSource}
            t={t}
          />
          <ShellPathChooser
            checked={shellPath}
            disabled={shellPathDisabled}
            onChange={onShellPath}
            t={t}
          />
        </div>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function SourceChooser({
  sources,
  value,
  onChange,
  t,
}: {
  sources: StartkitManifestSummary["sources"];
  value: string;
  onChange: (value: string) => void;
  t: (key: string, params?: Record<string, string | number>) => string;
}) {
  const entries: Array<[string, { label: string }]> =
    Object.keys(sources).length > 0
      ? Object.entries(sources)
      : [
          ["global", { label: "Global" }],
          ["cn", { label: "China mirror" }],
        ];

  return (
    <div>
      <div className="mb-2 flex items-center gap-2 text-xs font-medium">
        <Globe className="h-3.5 w-3.5 text-primary" />
        {t("Node/npm source")}
      </div>
      <div className="grid grid-cols-2 gap-2">
        {entries.map(([id, source]) => (
          <Button
            key={id}
            type="button"
            size="sm"
            variant="outline"
            className={cn(
              "justify-center text-xs",
              value === id && "border-primary bg-primary/10 text-primary",
            )}
            onClick={() => onChange(id)}
          >
            {t(source.label)}
          </Button>
        ))}
      </div>
    </div>
  );
}

function ShellPathChooser({
  checked,
  disabled,
  onChange,
  t,
}: {
  checked: boolean;
  disabled: boolean;
  onChange: (checked: boolean) => void;
  t: (key: string, params?: Record<string, string | number>) => string;
}) {
  return (
    <div
      className={cn(
        "pt-1",
        disabled && "opacity-60",
      )}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2 text-xs font-medium">
            <TerminalSquare className="h-3.5 w-3.5 text-primary" />
            {t("Write shell PATH")}
          </div>
          <p className="mt-1 text-[11px] leading-snug text-muted-foreground">
            {t("Terminal sessions can find managed Node, Codex, Claude, and helper tools.")}
          </p>
        </div>
        <Switch
          checked={checked}
          disabled={disabled}
          onCheckedChange={onChange}
          aria-label={t("Write shell PATH")}
        />
      </div>
    </div>
  );
}
