import { useRef, type KeyboardEvent, type PointerEvent } from "react";

import { cn } from "@/lib/utils";
import {
  MAX_PREVIEW_CHAT_WIDTH,
  MIN_PREVIEW_CHAT_WIDTH,
  clampPreviewChatWidth,
  resizePreviewChatWidth,
  type PreviewChatSide,
} from "./previewChatLayout";

type PreviewChatResizeHandleProps = {
  side: PreviewChatSide;
  width: number;
  onWidthChange: (width: number) => void;
};

type Drag = {
  pointerId: number;
  startX: number;
  startWidth: number;
};

export function PreviewChatResizeHandle({
  side,
  width,
  onWidthChange,
}: PreviewChatResizeHandleProps) {
  const drag = useRef<Drag | undefined>(undefined);

  const startDrag = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    drag.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startWidth: clampPreviewChatWidth(width),
    };
    event.currentTarget.setPointerCapture(event.pointerId);
    event.preventDefault();
  };

  const continueDrag = (event: PointerEvent<HTMLDivElement>) => {
    const active = drag.current;
    if (!active || active.pointerId !== event.pointerId) return;
    onWidthChange(
      resizePreviewChatWidth(
        active.startWidth,
        event.clientX - active.startX,
        side,
      ),
    );
  };

  const finishDrag = (event: PointerEvent<HTMLDivElement>) => {
    if (drag.current?.pointerId !== event.pointerId) return;
    drag.current = undefined;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };

  const resizeWithKeyboard = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Home") {
      onWidthChange(MIN_PREVIEW_CHAT_WIDTH);
    } else if (event.key === "End") {
      onWidthChange(MAX_PREVIEW_CHAT_WIDTH);
    } else if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
      const movement = event.key === "ArrowLeft" ? -8 : 8;
      onWidthChange(resizePreviewChatWidth(width, movement, side));
    } else {
      return;
    }
    event.preventDefault();
  };

  return (
    <div
      role="separator"
      tabIndex={0}
      aria-label="Resize preview conversation"
      aria-orientation="vertical"
      aria-valuemin={MIN_PREVIEW_CHAT_WIDTH}
      aria-valuemax={MAX_PREVIEW_CHAT_WIDTH}
      aria-valuenow={clampPreviewChatWidth(width)}
      onPointerDown={startDrag}
      onPointerMove={continueDrag}
      onPointerUp={finishDrag}
      onPointerCancel={finishDrag}
      onKeyDown={resizeWithKeyboard}
      className={cn(
        "absolute inset-y-0 z-10 hidden w-2 cursor-col-resize touch-none outline-none after:absolute after:inset-y-0 after:left-1/2 after:w-px after:-translate-x-1/2 after:bg-transparent hover:after:bg-primary/40 focus-visible:after:bg-primary lg:block",
        side === "left" ? "right-0" : "left-0",
      )}
    />
  );
}
