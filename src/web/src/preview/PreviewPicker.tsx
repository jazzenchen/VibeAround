import { Check, ChevronDown, Monitor } from "lucide-react";

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
          size="sm"
          className={cn(
            "min-w-0 max-w-72 flex-initial shrink justify-between gap-2 px-2.5 font-normal shadow-none",
            className,
          )}
          aria-label="Workspace and preview"
        >
          <span className="min-w-0 truncate font-medium text-foreground">
            {selected.title}
          </span>
          <ChevronDown className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-[min(20rem,calc(100vw-2rem))]">
        {Array.from(groups, ([workspace, items]) => (
          <div key={workspace}>
            <DropdownMenuLabel
              className="truncate text-xs text-muted-foreground"
              title={workspace}
            >
              {workspaceLabel(workspace)}
            </DropdownMenuLabel>
            {items.map((preview) => (
              <DropdownMenuItem
                key={preview.slug}
                onSelect={() => onSelect(preview.slug)}
                className="min-w-0"
              >
                <Monitor className="h-4 w-4" />
                <span className="min-w-0 flex-1 truncate">{preview.title}</span>
                {preview.slug === selected.slug && (
                  <Check className="h-4 w-4 text-primary" />
                )}
              </DropdownMenuItem>
            ))}
          </div>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
