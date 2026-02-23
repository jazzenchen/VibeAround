"use client";

import { useEffect, useRef } from "react";
import { Send, Square } from "lucide-react";
import type { ToolType } from "@/lib/terminal-types";
import { toolThemes } from "@/lib/terminal-types";
import { Button } from "@/components/ui/button";

const TEXTAREA_MAX_HEIGHT_PX = 128;

export interface ChatInputProps {
  value: string;
  onChange: (value: string) => void;
  onSubmit: () => void;
  disabled?: boolean;
  isStreaming?: boolean;
  onStop?: () => void;
  placeholder?: string;
  /** Shown at bottom-left as "Chat with {targetLabel}", colored by targetTool. */
  targetLabel?: string;
  /** Tool type for accent color (claude/gemini/codex/generic). */
  targetTool?: ToolType;
  className?: string;
}

export function ChatInput({
  value,
  onChange,
  onSubmit,
  disabled = false,
  isStreaming = false,
  onStop,
  placeholder = "Message Claude…",
  targetLabel = "Claude Code",
  targetTool = "claude",
  className,
}: ChatInputProps) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, TEXTAREA_MAX_HEIGHT_PX)}px`;
  }, [value]);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      if (!disabled && value.trim()) onSubmit();
    }
  };

  const canSend = !disabled && !!value.trim();
  const showStop = isStreaming && onStop;
  const accentColor = toolThemes[targetTool].accent;

  // One bordered group: textarea (grows) + addon bar with button inside. Focus on textarea highlights the whole box (like Apple Data Analysis Demo input-group).
  return (
    <div
      data-slot="chat-input"
      className={`bg-background p-4 ${className ?? ""}`}
      style={{ borderTop: "1px solid oklch(0.20 0.01 260)" }}
    >
      <div
        role="group"
        className="flex min-h-[2.5rem] flex-col rounded-lg border border-border bg-muted/30 transition-[box-shadow,border-color] focus-within:border-primary/50 focus-within:ring-2 focus-within:ring-primary/30"
      >
        <textarea
          ref={textareaRef}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder={placeholder}
          disabled={disabled}
          rows={1}
          className="min-h-[2.5rem] max-h-32 resize-none overflow-y-auto border-0 bg-transparent px-3 py-2 text-base sm:text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-0 transition-[height] duration-200 ease-out"
          style={{ height: "2.5rem" }}
        />
        <div className="flex shrink-0 items-center justify-between gap-2 px-2 py-1.5">
          <span className="flex items-center gap-1 truncate min-w-0 text-xs font-medium" title={`Chat with ${targetLabel}`}>
            <span className="text-muted-foreground shrink-0">Chat with</span>
            <span className="truncate" style={{ color: accentColor }}>{targetLabel}</span>
          </span>
          <Button
            type="button"
            size="icon"
            onClick={showStop ? onStop : onSubmit}
            disabled={!showStop && !canSend}
            className="h-8 w-8 shrink-0 rounded-full"
            aria-label={showStop ? "Stop" : "Send"}
          >
            {showStop ? (
              <Square className="h-4 w-4" />
            ) : (
              <Send className="h-4 w-4" />
            )}
          </Button>
        </div>
      </div>
    </div>
  );
}
