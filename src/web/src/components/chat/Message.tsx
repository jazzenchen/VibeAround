"use client";

import type { HTMLAttributes } from "react";

export type MessageRole = "user" | "assistant" | "system";

export type MessageProps = HTMLAttributes<HTMLDivElement> & {
  from: MessageRole;
};

export function Message({ className, from, ...props }: MessageProps) {
  return (
    <div
      className={`group flex w-full max-w-[95%] flex-col gap-2 ${
        from === "user"
          ? "is-user ml-auto items-end"
          : from === "system"
            ? "is-system mx-auto items-center"
            : "is-assistant items-start"
      } ${className ?? ""}`}
      {...props}
    />
  );
}

export type MessageContentProps = HTMLAttributes<HTMLDivElement>;

export function MessageContent({ children, className, ...props }: MessageContentProps) {
  return (
    <div
      className={`flex min-w-0 max-w-full flex-col gap-2 overflow-hidden text-sm rounded-lg px-4 py-3 group-[.is-user]:ml-auto group-[.is-user]:bg-primary/15 group-[.is-assistant]:bg-muted/50 ${className ?? ""}`}
      {...props}
    >
      {children}
    </div>
  );
}
