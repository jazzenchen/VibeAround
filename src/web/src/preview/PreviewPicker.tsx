import { Check, ChevronDown, FileText, Monitor } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";
import type { PreviewItem } from "./previewTypes";

type PreviewPickerProps = {
  previews: PreviewItem[];
  selected: PreviewItem;
  onSelect: (slug: string) => void;
  className?: string;
};

function workspaceLabel(path: string) {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

function kindLabel(preview: PreviewItem) {
  const kind = preview.kind.toLowerCase();
  return kind === "file" || kind.includes("markdown") ? "Markdown" : "Web";
}

export function PreviewPicker({
  previews,
  selected,
  onSelect,
  className,
}: PreviewPickerProps) {
  const groups = new Map<string, PreviewItem[]>();
  for (const preview of previews) {
    const group = groups.get(preview.workspace) ?? [];
    group.push(preview);
    groups.set(preview.workspace, group);
  }

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="outline"
          className={cn(
            "min-w-0 max-w-80 flex-initial shrink justify-between gap-3 px-3 font-normal shadow-none",
            className,
          )}
          aria-label="Workspace and preview"
        >
          <span className="min-w-0 truncate">
            <span className="font-medium text-foreground">{selected.title}</span>
            <span className="text-muted-foreground"> · {kindLabel(selected)}</span>
          </span>
          <ChevronDown className="h-4 w-4 shrink-0 text-muted-foreground" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-[min(24rem,calc(100vw-2rem))]">
        {Array.from(groups, ([workspace, items]) => (
          <div key={workspace}>
            <DropdownMenuLabel
              className="truncate text-xs text-muted-foreground"
              title={workspace}
            >
              {workspaceLabel(workspace)}
            </DropdownMenuLabel>
            {items.map((preview) => {
              const kind = preview.kind.toLowerCase();
              const Icon = kind === "file" || kind.includes("markdown")
                ? FileText
                : Monitor;
              return (
                <DropdownMenuItem
                  key={preview.slug}
                  onSelect={() => onSelect(preview.slug)}
                  className="min-w-0"
                >
                  <Icon className="h-4 w-4" />
                  <span className="min-w-0 flex-1 truncate">{preview.title}</span>
                  <span className="text-xs text-muted-foreground">
                    {kindLabel(preview)}
                  </span>
                  {preview.slug === selected.slug && (
                    <Check className="h-4 w-4 text-primary" />
                  )}
                </DropdownMenuItem>
              );
            })}
          </div>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
