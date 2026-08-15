import { useRef, type PointerEvent as ReactPointerEvent } from "react";
import {
  PanelsTopLeft,
  PictureInPicture2,
  RefreshCw,
  X,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import type { PreviewChatMode, PreviewChatSide } from "./previewChatLayout";
import { PREVIEW_SURFACE_DRAG_THRESHOLD } from "./previewHelperPosition";
import { PreviewPicker } from "./PreviewPicker";
import type { PreviewItem } from "./previewTypes";

type PreviewChatHeaderProps = {
  previews: PreviewItem[];
  selected: PreviewItem;
  mode: PreviewChatMode;
  onSelectPreview: (slug: string) => void;
  onRefresh: () => void;
  onModeChange: (mode: PreviewChatMode) => void;
  onSideChange: (side: PreviewChatSide) => void;
  onClose: () => void;
};

type TitleDragSession = {
  pointerId: number;
  startX: number;
  panelLeft: number;
  panelWidth: number;
  panel: HTMLElement;
  moved: boolean;
};

function titleDragOffset(session: TitleDragSession, clientX: number) {
  return Math.min(
    window.innerWidth - session.panelLeft - session.panelWidth,
    Math.max(-session.panelLeft, clientX - session.startX),
  );
}

function clearTitleDragStyles(panel: HTMLElement) {
  panel.style.removeProperty("transform");
  panel.style.removeProperty("will-change");
}

export function PreviewChatHeader({
  previews,
  selected,
  mode,
  onSelectPreview,
  onRefresh,
  onModeChange,
  onSideChange,
  onClose,
}: PreviewChatHeaderProps) {
  const dragSessionRef = useRef<TitleDragSession | undefined>(undefined);
  const nextMode = mode === "floating" ? "impact" : "floating";

  const startTitleDrag = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (
      event.button !== 0 ||
      !window.matchMedia("(min-width: 1024px)").matches
    ) {
      return;
    }
    const panel = event.currentTarget.closest("aside");
    if (!panel) return;
    const rect = panel.getBoundingClientRect();
    dragSessionRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      panelLeft: rect.left,
      panelWidth: rect.width,
      panel,
      moved: false,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const moveTitleDrag = (event: ReactPointerEvent<HTMLDivElement>) => {
    const session = dragSessionRef.current;
    if (!session || session.pointerId !== event.pointerId) return;
    const offset = titleDragOffset(session, event.clientX);
    if (
      !session.moved &&
      Math.abs(offset) < PREVIEW_SURFACE_DRAG_THRESHOLD
    ) {
      return;
    }
    session.moved = true;
    session.panel.style.transform = `translate3d(${offset}px, 0, 0)`;
    session.panel.style.willChange = "transform";
  };

  const finishTitleDrag = (event: ReactPointerEvent<HTMLDivElement>) => {
    const session = dragSessionRef.current;
    if (!session || session.pointerId !== event.pointerId) return;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    dragSessionRef.current = undefined;
    const offset = titleDragOffset(session, event.clientX);
    if (
      !session.moved &&
      Math.abs(offset) < PREVIEW_SURFACE_DRAG_THRESHOLD
    ) {
      return;
    }
    const panelCenter =
      session.panelLeft + offset + session.panelWidth / 2;
    onSideChange(panelCenter < window.innerWidth / 2 ? "left" : "right");
    clearTitleDragStyles(session.panel);
  };

  const cancelTitleDrag = (event: ReactPointerEvent<HTMLDivElement>) => {
    const session = dragSessionRef.current;
    if (!session || session.pointerId !== event.pointerId) return;
    dragSessionRef.current = undefined;
    clearTitleDragStyles(session.panel);
  };

  return (
    <header className="flex h-11 shrink-0 items-center gap-1.5 border-b border-border px-2">
      <div
        className="flex shrink-0 touch-none select-none items-center gap-2 lg:cursor-grab lg:active:cursor-grabbing"
        title="Drag to move Preview conversation"
        onPointerDown={startTitleDrag}
        onPointerMove={moveTitleDrag}
        onPointerUp={finishTitleDrag}
        onPointerCancel={cancelTitleDrag}
      >
        <img
          src="/va/brand/vibearound-mark.svg"
          alt=""
          draggable={false}
        className="pointer-events-none h-5 w-5 shrink-0"
        />
        <span className="hidden text-sm font-semibold sm:inline">Preview</span>
      </div>
      <PreviewPicker
        previews={previews}
        selected={selected}
        onSelect={onSelectPreview}
        className="h-8 min-w-0 max-w-52 flex-1 px-2"
      />
      <Button
        type="button"
        variant="ghost"
        size="icon-sm"
        onClick={onRefresh}
        aria-label="Refresh preview"
        title="Refresh preview"
      >
        <RefreshCw className="h-4 w-4" />
      </Button>
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
        onClick={onClose}
        aria-label="Close conversation"
        title="Close conversation"
      >
        <X className="h-4 w-4" />
      </Button>
    </header>
  );
}
