import {
  Globe,
  Package,
  SlidersHorizontal,
  Wrench,
} from "lucide-react";
import type { ReactNode } from "react";
import { useI18n } from "@va/i18n";

import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
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
          <InstallPathChooser
            value={toolchainMode}
            onChange={onToolchainMode}
            t={t}
          />
          <PortableToolchainChooser
            checked={portableToolchain}
            onChange={onPortableToolchain}
            t={t}
          />
        </div>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function InstallPathChooser({
  value,
  onChange,
  t,
}: {
  value: ToolchainMode;
  onChange: (value: ToolchainMode) => void;
  t: (key: string, params?: Record<string, string | number>) => string;
}) {
  return (
    <SegmentedChooser
      icon={<Package className="h-3.5 w-3.5 text-primary" />}
      label={t("Install Path")}
      options={[
        { value: "system", label: t("System") },
        { value: "managed", label: t("Managed") },
      ]}
      value={value}
      onChange={(next) => onChange(next as ToolchainMode)}
    />
  );
}

function PortableToolchainChooser({
  checked,
  onChange,
  t,
}: {
  checked: boolean;
  onChange: (value: boolean) => void;
  t: (key: string, params?: Record<string, string | number>) => string;
}) {
  return (
    <SegmentedChooser
      icon={<Wrench className="h-3.5 w-3.5 text-primary" />}
      label={t("Toolchain")}
      options={[
        { value: "system", label: t("System") },
        { value: "portable", label: t("Portable") },
      ]}
      value={checked ? "portable" : "system"}
      onChange={(next) => onChange(next === "portable")}
    />
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
    <SegmentedChooser
      icon={<Globe className="h-3.5 w-3.5 text-primary" />}
      label={t("npm registry")}
      options={entries.map(([id, source]) => ({
        value: id,
        label: t(source.label),
      }))}
      value={value}
      onChange={onChange}
    />
  );
}

function SegmentedChooser({
  icon,
  label,
  options,
  value,
  onChange,
}: {
  icon: ReactNode;
  label: string;
  options: Array<{ value: string; label: string }>;
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <div>
      <div className="mb-2 flex items-center gap-2 text-xs font-medium">
        {icon}
        {label}
      </div>
      <div className="grid grid-cols-2 gap-2">
        {options.map((option) => (
          <Button
            key={option.value}
            type="button"
            size="sm"
            variant="outline"
            className={cn(
              "justify-center text-xs",
              value === option.value &&
                "border-primary bg-primary/10 text-primary",
            )}
            onClick={() => onChange(option.value)}
          >
            {option.label}
          </Button>
        ))}
      </div>
    </div>
  );
}
