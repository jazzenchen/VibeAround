"use client";

import { useEffect, useRef } from "react";
import { ChevronDown, Send, Square } from "lucide-react";
import type { ToolType } from "@/lib/terminal-types";
import { getToolTheme } from "@/lib/terminal-types";
import { useTheme } from "@/lib/theme";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { AgentInfo } from "@/api/agents";

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
  /** Available agents for the selector dropdown. */
  agents?: AgentInfo[];
  /** Called when user picks a different agent from the dropdown. */
  onAgentChange?: (agentId: string) => void;
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
  agents,
  onAgentChange,
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
  const appTheme = useTheme();
  const accentColor = getToolTheme(targetTool, appTheme).accent;

  const hasMultipleAgents = agents && agents.length > 1 && onAgentChange;

  return (
    <div
      data-slot="chat-input"
      className={`bg-background p-4 border-t border-border ${className ?? ""}`}
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
          {hasMultipleAgents ? (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button
                  type="button"
                  className="flex items-center gap-1 truncate min-w-0 text-xs font-medium cursor-pointer rounded px-1 py-0.5 hover:bg-muted/60 transition-colors"
                  title={`Chat with ${targetLabel}`}
                >
                  <span className="text-muted-foreground shrink-0">Chat with</span>
                  <span className="truncate" style={{ color: accentColor }}>{targetLabel}</span>
                  <ChevronDown className="h-3 w-3 shrink-0 text-muted-foreground" />
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent side="top" align="start" className="min-w-[160px]">
                {agents!.map((agent) => (
                  <DropdownMenuItem
                    key={agent.id}
                    onClick={() => onAgentChange!(agent.id)}
                    className="flex items-center justify-between gap-2"
                  >
                    <span className="capitalize">{agent.id}</span>
                    {agent.id === targetTool && (
                      <span className="text-xs text-muted-foreground">current</span>
                    )}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          ) : (
            <span className="flex items-center gap-1 truncate min-w-0 text-xs font-medium" title={`Chat with ${targetLabel}`}>
              <span className="text-muted-foreground shrink-0">Chat with</span>
              <span className="truncate" style={{ color: accentColor }}>{targetLabel}</span>
            </span>
          )}
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
