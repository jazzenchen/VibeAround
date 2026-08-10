import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { Check, X } from "lucide-react";

import { Button } from "@/components/ui/button";
import { previewAnchorLocation, previewAnchorQuote } from "./previewReview";
import type { PreviewReviewEditor } from "./usePreviewReviewBridge";

type PreviewReviewPopoverProps = {
  editor: PreviewReviewEditor;
  frameRef: RefObject<HTMLIFrameElement | null>;
  initialComment: string;
  onSave: (comment: string) => void;
  onCancel: () => void;
};

type Position = { left: number; top: number; width: number };

export function PreviewReviewPopover({
  editor,
  frameRef,
  initialComment,
  onSave,
  onCancel,
}: PreviewReviewPopoverProps) {
  const [comment, setComment] = useState(initialComment);
  const [position, setPosition] = useState<Position>();
  const popoverRef = useRef<HTMLFormElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    setComment(initialComment);
    requestAnimationFrame(() => inputRef.current?.focus());
  }, [editor.anchorId, initialComment]);

  useLayoutEffect(() => {
    const positionPopover = () => {
      const frame = frameRef.current;
      const popover = popoverRef.current;
      if (!frame || !popover) return;
      const frameRect = frame.getBoundingClientRect();
      const width = Math.min(360, window.innerWidth - 24);
      const height = popover.offsetHeight;
      const anchorLeft = frameRect.left + editor.rect.x;
      const anchorTop = frameRect.top + editor.rect.y;
      let top = anchorTop + editor.rect.height + 8;
      if (top + height > window.innerHeight - 12) {
        top = anchorTop - height - 8;
      }
      setPosition({
        left: Math.max(12, Math.min(anchorLeft, window.innerWidth - width - 12)),
        top: Math.max(12, top),
        width,
      });
    };

    positionPopover();
    window.addEventListener("resize", positionPopover);
    return () => window.removeEventListener("resize", positionPopover);
  }, [editor.rect, frameRef]);

  useEffect(() => {
    const handlePointerDown = (event: PointerEvent) => {
      if (!popoverRef.current?.contains(event.target as Node)) onCancel();
    };
    const handleFocus = (event: FocusEvent) => {
      if (!popoverRef.current?.contains(event.target as Node)) onCancel();
    };
    const handleWindowBlur = () => {
      setTimeout(() => {
        if (document.activeElement === frameRef.current) onCancel();
      }, 0);
    };
    document.addEventListener("pointerdown", handlePointerDown, true);
    document.addEventListener("focusin", handleFocus, true);
    window.addEventListener("blur", handleWindowBlur);
    return () => {
      document.removeEventListener("pointerdown", handlePointerDown, true);
      document.removeEventListener("focusin", handleFocus, true);
      window.removeEventListener("blur", handleWindowBlur);
    };
  }, [frameRef, onCancel]);

  const submit = () => {
    const value = comment.trim();
    if (value) onSave(value);
    else inputRef.current?.focus();
  };

  return (
    <form
      ref={popoverRef}
      className="fixed z-50 rounded-lg border border-border bg-popover p-2.5 text-popover-foreground shadow-xl"
      style={{ ...position, visibility: position ? "visible" : "hidden" }}
      onSubmit={(event) => {
        event.preventDefault();
        submit();
      }}
    >
      <Button
        type="button"
        variant="ghost"
        size="icon-xs"
        onClick={onCancel}
        className="absolute right-1.5 top-1.5 bg-transparent text-muted-foreground hover:bg-transparent hover:text-foreground"
        aria-label="Cancel comment"
        title="Cancel"
      >
        <X className="h-3.5 w-3.5" />
      </Button>
      <div className="pr-7 text-[11px] font-medium text-muted-foreground">
        {previewAnchorLocation(editor.anchor)}
      </div>
      <blockquote className="mt-1 line-clamp-3 border-l-2 border-border pl-2 text-xs text-muted-foreground">
        {previewAnchorQuote(editor.anchor)}
      </blockquote>
      <div className="relative mt-2 rounded-md border border-border bg-background focus-within:border-primary/50 focus-within:ring-2 focus-within:ring-primary/20">
        <textarea
          ref={inputRef}
          value={comment}
          onChange={(event) => setComment(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
              event.preventDefault();
              submit();
            }
          }}
          rows={3}
          maxLength={2000}
          placeholder="What should change?"
          className="min-h-20 w-full resize-none bg-transparent px-2.5 py-2 pr-10 text-sm outline-none placeholder:text-muted-foreground"
        />
        <Button
          type="submit"
          size="icon-xs"
          className="absolute bottom-2 right-2 rounded-full"
          aria-label="Save comment"
          title="Save"
        >
          <Check className="h-3.5 w-3.5" />
        </Button>
      </div>
    </form>
  );
}
