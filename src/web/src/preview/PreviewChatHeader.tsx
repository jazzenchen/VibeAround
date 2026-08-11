import {
  PanelLeft,
  PanelRight,
  PanelsTopLeft,
  PictureInPicture2,
  X,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import type { PreviewChatMode, PreviewChatSide } from "./previewChatLayout";

type PreviewChatHeaderProps = {
  subtitle: string;
  mode: PreviewChatMode;
  side: PreviewChatSide;
  onModeChange: (mode: PreviewChatMode) => void;
  onSideChange: (side: PreviewChatSide) => void;
  onClose: () => void;
};

export function PreviewChatHeader({
  subtitle,
  mode,
  side,
  onModeChange,
  onSideChange,
  onClose,
}: PreviewChatHeaderProps) {
  const nextMode = mode === "floating" ? "impact" : "floating";
  const nextSide = side === "left" ? "right" : "left";

  return (
    <header className="flex h-14 shrink-0 items-center gap-2 border-b border-border px-3">
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm font-semibold">Preview conversation</div>
        <div className="truncate text-xs text-muted-foreground">{subtitle}</div>
      </div>
      <Button
        type="button"
        variant="ghost"
        size="icon-sm"
        onClick={() => onModeChange(nextMode)}
        aria-label={`Use ${nextMode} layout`}
        title={`Use ${nextMode} layout`}
        className="hidden lg:inline-flex"
      >
        {nextMode === "floating" ? (
          <PictureInPicture2 className="h-4 w-4" />
        ) : (
          <PanelsTopLeft className="h-4 w-4" />
        )}
      </Button>
      <Button
        type="button"
        variant="ghost"
        size="icon-sm"
        onClick={() => onSideChange(nextSide)}
        aria-label={`Move conversation to the ${nextSide}`}
        title={`Move conversation to the ${nextSide}`}
        className="hidden lg:inline-flex"
      >
        {nextSide === "left" ? (
          <PanelLeft className="h-4 w-4" />
        ) : (
          <PanelRight className="h-4 w-4" />
        )}
      </Button>
      <Button
        type="button"
        variant="ghost"
        size="icon-sm"
        onClick={onClose}
        aria-label="Close conversation"
        title="Close conversation"
      >
        <X className="h-4 w-4" />
      </Button>
    </header>
  );
}
