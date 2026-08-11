import { useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { MessageSquare, Minimize2, RefreshCw } from "lucide-react";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import {
  hasPreviewHelperDragStarted,
  nearestPreviewHelperCorner,
  type PreviewHelperCorner,
} from "./previewHelperPosition";
import { PreviewPicker } from "./PreviewPicker";
import type { PreviewItem } from "./previewTypes";

type PreviewHelperProps = {
  open: boolean;
  chatOpen: boolean;
  corner: PreviewHelperCorner;
  previews: PreviewItem[];
  selected: PreviewItem;
  onOpenChange: (open: boolean) => void;
  onChatOpenChange: (open: boolean) => void;
  onCornerChange: (corner: PreviewHelperCorner) => void;
  onSelectPreview: (slug: string) => void;
  onRefresh: () => void;
};

type DragSession = {
  pointerId: number;
  startX: number;
  startY: number;
  startLeft: number;
  startTop: number;
  width: number;
  height: number;
  suppressClick: boolean;
  moved: boolean;
};

type DragPosition = {
  left: number;
  top: number;
};

const CORNER_CLASS: Record<PreviewHelperCorner, string> = {
  "top-left": "left-5 top-5",
  "top-right": "left-[calc(100vw-1.25rem)] top-5 -translate-x-full",
  "bottom-left": "left-5 top-[calc(100vh-1.25rem)] -translate-y-full",
  "bottom-right":
    "left-[calc(100vw-1.25rem)] top-[calc(100vh-1.25rem)] -translate-x-full -translate-y-full",
};

export function PreviewHelper({
  open,
  chatOpen,
  corner,
  previews,
  selected,
  onOpenChange,
  onChatOpenChange,
  onCornerChange,
  onSelectPreview,
  onRefresh,
}: PreviewHelperProps) {
  const rootRef = useRef<HTMLDivElement>(null);
  const dragSessionRef = useRef<DragSession | undefined>(undefined);
  const didDragRef = useRef(false);
  const [dragPosition, setDragPosition] = useState<DragPosition>();

  const handlePointerDown = (event: ReactPointerEvent<HTMLElement>) => {
    if (event.button !== 0 || !rootRef.current) return;
    const rect = rootRef.current.getBoundingClientRect();
    didDragRef.current = false;
    dragSessionRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      startLeft: rect.left,
      startTop: rect.top,
      width: rect.width,
      height: rect.height,
      suppressClick: event.currentTarget.tagName === "BUTTON",
      moved: false,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const handlePointerMove = (event: ReactPointerEvent<HTMLElement>) => {
    const session = dragSessionRef.current;
    if (!session || session.pointerId !== event.pointerId) return;
    if (
      !session.moved &&
      !hasPreviewHelperDragStarted(
        { x: session.startX, y: session.startY },
        { x: event.clientX, y: event.clientY },
      )
    ) {
      return;
    }
    session.moved = true;
    didDragRef.current = session.suppressClick;
    setDragPosition({
      left: session.startLeft + event.clientX - session.startX,
      top: session.startTop + event.clientY - session.startY,
    });
  };

  const finishDrag = (event: ReactPointerEvent<HTMLElement>) => {
    const session = dragSessionRef.current;
    if (!session || session.pointerId !== event.pointerId) return;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    dragSessionRef.current = undefined;
    if (!session.moved) return;

    const left = session.startLeft + event.clientX - session.startX;
    const top = session.startTop + event.clientY - session.startY;
    onCornerChange(
      nearestPreviewHelperCorner(
        { left, top, width: session.width, height: session.height },
        { width: window.innerWidth, height: window.innerHeight },
      ),
    );
    setDragPosition(undefined);
  };

  const cancelDrag = (event: ReactPointerEvent<HTMLElement>) => {
    const session = dragSessionRef.current;
    if (!session || session.pointerId !== event.pointerId) return;
    dragSessionRef.current = undefined;
    didDragRef.current = false;
    setDragPosition(undefined);
  };

  const pointerHandlers = {
    onPointerDown: handlePointerDown,
    onPointerMove: handlePointerMove,
    onPointerUp: finishDrag,
    onPointerCancel: cancelDrag,
  };

  return (
    <div
      ref={rootRef}
      className={cn(
        "fixed z-50 select-none",
        chatOpen && "hidden lg:block",
        dragPosition
          ? "translate-x-0 translate-y-0 transition-none"
          : cn(
              "transition-[left,top,translate] duration-200 ease-out",
              CORNER_CLASS[corner],
            ),
      )}
      style={dragPosition}
    >
      {!open ? (
        <Button
          type="button"
          variant="outline"
          className="h-10 touch-none cursor-grab rounded-full border-border/80 bg-background/95 px-2.5 shadow-lg backdrop-blur active:cursor-grabbing"
          aria-label="Open Preview helper"
          title="Open Preview helper · drag to move"
          onClick={() => {
            if (didDragRef.current) {
              didDragRef.current = false;
              return;
            }
            onOpenChange(true);
          }}
          {...pointerHandlers}
        >
          <img
            src="/va/brand/vibearound-mark.svg"
            alt=""
            draggable={false}
            className="pointer-events-none h-5 w-5"
          />
          <span>Preview</span>
        </Button>
      ) : (
        <div
          className={cn(
            "flex h-11 items-center gap-1 rounded-xl border border-border/80 bg-background/95 p-1.5 shadow-lg backdrop-blur",
            chatOpen
              ? "w-[min(24rem,calc(100vw-2rem))]"
              : "w-[min(34rem,calc(100vw-2rem))]",
          )}
        >
          <div
            className="flex h-8 w-8 shrink-0 touch-none cursor-grab items-center justify-center rounded-md active:cursor-grabbing"
            title="Drag to move Preview helper"
            {...pointerHandlers}
          >
            <img
              src="/va/brand/vibearound-mark.svg"
              alt=""
              draggable={false}
              className="pointer-events-none h-5 w-5"
            />
          </div>
          <PreviewPicker
            previews={previews}
            selected={selected}
            onSelect={onSelectPreview}
            className="h-8 max-w-none flex-1 px-2"
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
            variant={chatOpen ? "secondary" : "ghost"}
            size="sm"
            className="px-2"
            onClick={() => onChatOpenChange(!chatOpen)}
            aria-label="Preview conversation"
            aria-pressed={chatOpen}
            title="Preview conversation"
          >
            <MessageSquare className="h-4 w-4" />
            <span className="hidden sm:inline">Chat</span>
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            onClick={() => onOpenChange(false)}
            aria-label="Collapse Preview helper"
            title="Collapse Preview helper"
          >
            <Minimize2 className="h-4 w-4" />
          </Button>
        </div>
      )}
    </div>
  );
}

export type { PreviewHelperCorner } from "./previewHelperPosition";
