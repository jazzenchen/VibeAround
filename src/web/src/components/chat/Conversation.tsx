"use client";

import { StickToBottom, useStickToBottomContext } from "use-stick-to-bottom";
import type { ComponentProps } from "react";
import { useCallback } from "react";
import { ChevronDown } from "lucide-react";

export type ConversationProps = ComponentProps<typeof StickToBottom>;

export function Conversation({ className, ...props }: ConversationProps) {
  return (
    <StickToBottom
      className={`relative flex-1 overflow-y-hidden ${className ?? ""}`}
      initial="smooth"
      resize="smooth"
      role="log"
      {...props}
    />
  );
}

export type ConversationContentProps = ComponentProps<typeof StickToBottom.Content>;

export function ConversationContent({ className, ...props }: ConversationContentProps) {
  return (
    <StickToBottom.Content
      className={`flex flex-col gap-6 p-4 ${className ?? ""}`}
      {...props}
    />
  );
}

export type ConversationEmptyStateProps = ComponentProps<"div"> & {
  title?: string;
  description?: string;
  icon?: React.ReactNode;
};

export function ConversationEmptyState({
  className,
  title = "No messages yet",
  description = "Send a message to start a conversation with Claude.",
  icon,
  children,
  ...props
}: ConversationEmptyStateProps) {
  return (
    <div
      className={`flex size-full flex-col items-center justify-center gap-3 p-8 text-center ${className ?? ""}`}
      {...props}
    >
      {children ?? (
        <>
          {icon && <div className="text-muted-foreground">{icon}</div>}
          <div className="space-y-1">
            <h3 className="text-sm font-medium text-foreground">{title}</h3>
            {description && (
              <p className="text-sm text-muted-foreground">{description}</p>
            )}
          </div>
        </>
      )}
    </div>
  );
}

export function ConversationScrollButton({
  className,
  ...props
}: ComponentProps<"button">) {
  const { isAtBottom, scrollToBottom } = useStickToBottomContext();
  const handleClick = useCallback(() => scrollToBottom(), [scrollToBottom]);

  if (isAtBottom) return null;
  return (
    <button
      type="button"
      onClick={handleClick}
      className={`absolute bottom-4 left-1/2 -translate-x-1/2 rounded-full border border-border bg-background p-2 text-muted-foreground shadow hover:bg-muted hover:text-foreground ${className ?? ""}`}
      aria-label="Scroll to bottom"
      {...props}
    >
      <ChevronDown className="h-4 w-4" />
    </button>
  );
}
