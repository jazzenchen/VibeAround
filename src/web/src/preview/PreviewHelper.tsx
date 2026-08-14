import { useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { Maximize2, MessageSquare, Minimize2, RefreshCw } from "lucide-react";

import { Button } from "@/components/ui/button";
import { ReviewToolbar } from "@/components/review/ReviewToolbar";
import type { ReviewToolbarModel } from "@/components/review/reviewTypes";
import { cn } from "@/lib/utils";
import {
  hasPreviewHelperDragStarted,
  nearestPreviewHelperCorner,
  type PreviewHelperCorner,
} from "./previewHelperPosition";
import { PreviewPicker } from "./PreviewPicker";
import type { PreviewItem } from "./previewTypes";

export type PreviewHelperView = "collapsed" | "expanded" | "chat";

type PreviewHelperProps = {
  view: PreviewHelperView;
  corner: PreviewHelperCorner;
  previews: PreviewItem[];
  selected: PreviewItem;
  onViewChange: (view: PreviewHelperView) => void;
  onCornerChange: (corner: PreviewHelperCorner) => void;
  onSelectPreview: (slug: string) => void;
  onRefresh: () => void;
  reviewToolbar: ReviewToolbarModel;
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
  view,
  corner,
  previews,
  selected,
  onViewChange,
  onCornerChange,
  onSelectPreview,
  onRefresh,
  reviewToolbar,
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
    const nearestCorner = nearestPreviewHelperCorner(
      { left, top, width: session.width, height: session.height },
      { width: window.innerWidth, height: window.innerHeight },
    );
    const vertical = nearestCorner.startsWith("top") ? "top" : "bottom";
    const horizontal = corner.endsWith("left") ? "left" : "right";
    onCornerChange(
      view === "expanded"
        ? `${vertical}-${horizontal}`
        : nearestCorner,
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

  if (view === "chat") return null;

  return (
    <div
      ref={rootRef}
      className={cn(
        "fixed z-50 select-none",
        view === "collapsed" && !dragPosition &&
          "opacity-80 hover:opacity-100 focus-within:opacity-100",
        dragPosition
          ? "translate-x-0 translate-y-0 transition-none"
          : cn(
              "transition-[left,top,translate,opacity] duration-200 ease-out",
              CORNER_CLASS[corner],
            ),
      )}
      style={dragPosition}
    >
      <div
        className={cn(
          "inline-flex w-fit rounded-xl border border-border/80 bg-background/95 p-1 shadow-2xl shadow-foreground/15 backdrop-blur",
          view === "expanded" && "max-w-[calc(100vw-2.5rem)]",
        )}
      >
        {view === "collapsed" ? (
          <div className="flex h-8 items-center gap-1">
            <button
              type="button"
              className="inline-flex h-8 touch-none cursor-grab items-center gap-1.5 rounded-lg text-sm font-medium outline-none transition-colors hover:bg-accent focus-visible:ring-[3px] focus-visible:ring-ring/50 active:cursor-grabbing"
              aria-label="Open Preview helper"
              title="Open Preview helper · drag to move"
              onClick={() => {
                if (didDragRef.current) {
                  didDragRef.current = false;
                  return;
                }
                onViewChange("expanded");
              }}
              {...pointerHandlers}
            >
              <span className="flex h-7 w-7 shrink-0 items-center justify-center">
                <img
                  src="/va/brand/vibearound-mark.svg"
                  alt=""
                  draggable={false}
                  className="pointer-events-none h-5 w-5"
                />
              </span>
              <span className="px-0.5">Preview</span>
            </button>
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              className="cursor-pointer"
              onClick={() => onViewChange("expanded")}
              aria-label="Expand Preview helper"
              title="Expand Preview helper"
            >
              <Maximize2 className="h-4 w-4 text-muted-foreground" />
            </Button>
          </div>
        ) : (
          <div className="flex min-w-0 flex-col gap-1">
            <div className="flex h-8 min-w-0 items-center gap-1">
              <div
                className="flex h-7 w-7 shrink-0 touch-none cursor-grab items-center justify-center rounded-md active:cursor-grabbing"
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
                className="h-8 px-2"
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
                size="sm"
                className="px-2"
                onClick={() => onViewChange("chat")}
                aria-label="Preview conversation"
                title="Preview conversation"
              >
                <MessageSquare className="h-4 w-4" />
                <span className="hidden sm:inline">Chat</span>
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="icon-sm"
                onClick={() => onViewChange("collapsed")}
                aria-label="Collapse Preview helper"
                title="Collapse Preview helper"
              >
                <Minimize2 className="h-4 w-4" />
              </Button>
            </div>
            <ReviewToolbar
              {...reviewToolbar}
              className="border-t border-border/70 px-0.5 pt-1"
            />
          </div>
        )}
      </div>
    </div>
  );
}

export type { PreviewHelperCorner } from "./previewHelperPosition";
