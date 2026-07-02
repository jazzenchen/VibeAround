import {
  Globe,
  Package,
  SlidersHorizontal,
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
import type { ToolchainMode } from "../types";

export function StartkitAdvancedMenu({
  sources,
  downloadSource,
  toolchainMode,
  portableToolchain,
  onDownloadSource,
  onToolchainMode,
  onPortableToolchain,
}: {
  sources: StartkitManifestSummary["sources"];
  downloadSource: string;
  toolchainMode: ToolchainMode;
  portableToolchain: boolean;
  onDownloadSource: (value: string) => void;
  onToolchainMode: (value: ToolchainMode) => void;
  onPortableToolchain: (value: boolean) => void;
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
          <ToolchainChooser
            value={toolchainMode}
            onChange={onToolchainMode}
            t={t}
          />
          <PortableToolchainToggle
            checked={portableToolchain}
            onCheckedChange={onPortableToolchain}
            t={t}
          />
        </div>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function ToolchainChooser({
  value,
  onChange,
  t,
}: {
  value: ToolchainMode;
  onChange: (value: ToolchainMode) => void;
  t: (key: string, params?: Record<string, string | number>) => string;
}) {
  return (
    <div>
      <div className="mb-2 flex items-center gap-2 text-xs font-medium">
        <Package className="h-3.5 w-3.5 text-primary" />
        {t("Toolchain")}
      </div>
      <div className="grid grid-cols-2 gap-2">
        {(["system", "managed"] as const).map((mode) => (
          <Button
            key={mode}
            type="button"
            size="sm"
            variant="outline"
            className={cn(
              "justify-center text-xs",
              value === mode && "border-primary bg-primary/10 text-primary",
            )}
            onClick={() => onChange(mode)}
          >
            {mode === "system" ? t("System") : t("VibeAround managed")}
          </Button>
        ))}
      </div>
    </div>
  );
}

function PortableToolchainToggle({
  checked,
  onCheckedChange,
  t,
}: {
  checked: boolean;
  onCheckedChange: (value: boolean) => void;
  t: (key: string, params?: Record<string, string | number>) => string;
}) {
  const label = isWindowsPlatform()
    ? t("Use portable npm/Git")
    : t("Use portable npm");

  return (
    <div className="flex items-center justify-between gap-3 rounded-md border border-border/70 px-3 py-2.5">
      <div className="min-w-0 text-xs font-medium">{label}</div>
      <Switch
        checked={checked}
        onCheckedChange={onCheckedChange}
        aria-label={label}
        size="sm"
      />
    </div>
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
        {t("npm registry")}
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

function isWindowsPlatform() {
  return typeof navigator !== "undefined" && /Win/i.test(navigator.platform);
}
