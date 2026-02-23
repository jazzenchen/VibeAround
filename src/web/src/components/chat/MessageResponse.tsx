"use client";

import * as React from "react";
import { Streamdown } from "streamdown";
import { code } from "@streamdown/code";
import { cjk } from "@streamdown/cjk";
import type { ComponentProps } from "react";

export type MessageResponseProps = ComponentProps<typeof Streamdown> & {
  content: string;
  isStreaming?: boolean;
};

/** Renders assistant message with Streamdown (Markdown, code, CJK). */
export const MessageResponse = React.memo(
  ({ content, isStreaming = false, className, ...props }: MessageResponseProps) => (
    <Streamdown
      className={[
        "prose prose-sm dark:prose-invert max-w-none text-sm",
        "[&>*:first-child]:mt-0 [&>*:last-child]:mb-0",
        className ?? "",
      ]
        .filter(Boolean)
        .join(" ")}
      plugins={{ cjk, code }}
      shikiTheme={["github-light", "github-dark"]}
      isAnimating={isStreaming}
      parseIncompleteMarkdown={true}
      {...props}
    >
      {content}
    </Streamdown>
  ),
  (prev, next) => prev.content === next.content && prev.isStreaming === next.isStreaming
);
MessageResponse.displayName = "MessageResponse";
