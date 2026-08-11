import { Scan, ScanLine } from "lucide-react";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

export type PreviewReviewTool = "element" | "region";

export type PreviewReviewToolbarModel = {
  activeTool: PreviewReviewTool | null;
  elementAvailable: boolean;
  regionAvailable: boolean;
  textSelectionAvailable: boolean;
  onToolChange: (tool: PreviewReviewTool | null) => void;
};

type PreviewReviewToolbarProps = PreviewReviewToolbarModel & {
  className?: string;
};

export function PreviewReviewToolbar({
  activeTool,
  elementAvailable,
  regionAvailable,
  textSelectionAvailable,
  onToolChange,
  className,
}: PreviewReviewToolbarProps) {
  const toggle = (tool: PreviewReviewTool) => {
    onToolChange(activeTool === tool ? null : tool);
  };

  return (
    <div
      className={cn("flex min-w-0 items-center gap-1.5", className)}
      aria-label="Preview review tools"
    >
      <Button
        type="button"
        variant={activeTool === "element" ? "secondary" : "outline"}
        size="xs"
        disabled={!elementAvailable}
        aria-pressed={activeTool === "element"}
        onClick={() => toggle("element")}
      >
        <Scan className="size-3" />
        Element
      </Button>
      <Button
        type="button"
        variant={activeTool === "region" ? "secondary" : "outline"}
        size="xs"
        disabled={!regionAvailable}
        aria-pressed={activeTool === "region"}
        onClick={() => toggle("region")}
      >
        <ScanLine className="size-3" />
        Region
      </Button>
      {textSelectionAvailable && (
        <span className="min-w-0 truncate text-[11px] text-muted-foreground">
          or select page text
        </span>
      )}
    </div>
  );
}
