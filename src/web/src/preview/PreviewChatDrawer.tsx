import { useEffect, useState, type CSSProperties } from "react";
import { MessageSquare, MousePointer2, X } from "lucide-react";

import {
  ChatInput,
  ChatMessageList,
  PendingPermissions,
} from "@/components/chat/chatUi";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { PreviewChatHeader } from "./PreviewChatHeader";
import { PreviewChatResizeHandle } from "./PreviewChatResizeHandle";
import {
  clampPreviewChatWidth,
  type PreviewChatMode,
  type PreviewChatSide,
} from "./previewChatLayout";
import { buildPreviewReviewPrompt, previewReviewDisplay } from "./previewReview";
import type { PreviewItem, PreviewReviewDraft } from "./previewTypes";
import type { usePreviewChatConnection } from "./usePreviewChatConnection";

type PreviewChat = ReturnType<typeof usePreviewChatConnection>;
const MAX_PREVIEW_MESSAGE_LENGTH = 20_000;

type PreviewChatDrawerProps = {
  open: boolean;
  preview: PreviewItem;
  chat: PreviewChat;
  drafts: PreviewReviewDraft[];
  supportsElementSelection: boolean;
  elementSelectionActive: boolean;
  mode: PreviewChatMode;
  side: PreviewChatSide;
  width: number;
  onClose: () => void;
  onModeChange: (mode: PreviewChatMode) => void;
  onSideChange: (side: PreviewChatSide) => void;
  onWidthChange: (width: number) => void;
  onSelectElement: () => void;
  onFocusDraft: (id: string) => void;
  onRemoveDraft: (id: string) => void;
  onClearSubmittedDrafts: () => void;
};

function draftExcerpt(draft: PreviewReviewDraft) {
  const value = draft.anchor.text.replace(/\s+/g, " ").trim();
  return value.length > 38 ? `${value.slice(0, 37)}…` : value || "Selected element";
}

export function PreviewChatDrawer({
  open,
  preview,
  chat,
  drafts,
  supportsElementSelection,
  elementSelectionActive,
  mode,
  side,
  width,
  onClose,
  onModeChange,
  onSideChange,
  onWidthChange,
  onSelectElement,
  onFocusDraft,
  onRemoveDraft,
  onClearSubmittedDrafts,
}: PreviewChatDrawerProps) {
  const [input, setInput] = useState("");
  const [submitError, setSubmitError] = useState("");

  useEffect(() => {
    setInput("");
    setSubmitError("");
  }, [preview.slug]);

  const submit = () => {
    const prompt = input.trim();
    if (!prompt && drafts.length === 0) return;
    const message = drafts.length
      ? buildPreviewReviewPrompt(preview, drafts, prompt)
      : prompt;
    const display = drafts.length
      ? previewReviewDisplay(drafts, prompt)
      : prompt;
    if (message.length > MAX_PREVIEW_MESSAGE_LENGTH) {
      setSubmitError("These review notes are too long for one message.");
      return;
    }
    if (!chat.sendMessage(message, display)) {
      setSubmitError("Wait for the current turn to finish.");
      return;
    }
    setInput("");
    setSubmitError("");
    if (drafts.length) onClearSubmittedDrafts();
  };

  const drawerStyle = {
    "--preview-chat-width": `${clampPreviewChatWidth(width)}px`,
  } as CSSProperties;

  return (
    <aside
      aria-label="Preview conversation"
      style={drawerStyle}
      className={cn(
        "z-40 min-h-0 w-full flex-col overflow-hidden border-border bg-background lg:w-[var(--preview-chat-width)]",
        open ? "flex" : "hidden",
        mode === "floating"
          ? "fixed inset-0 shadow-2xl lg:inset-y-3 lg:h-auto lg:rounded-xl lg:border"
          : "fixed inset-0 lg:relative lg:inset-auto lg:h-full lg:shrink-0 lg:shadow-none",
        mode === "floating" && side === "left" &&
          "lg:left-3 lg:right-auto",
        mode === "floating" && side === "right" &&
          "lg:left-auto lg:right-3",
        mode === "impact" && side === "left" &&
          "lg:order-first lg:border-r",
        mode === "impact" && side === "right" &&
          "lg:order-last lg:border-l",
      )}
    >
      <PreviewChatResizeHandle
        side={side}
        width={width}
        onWidthChange={onWidthChange}
      />
      <PreviewChatHeader
        subtitle={
          !preview.chatAvailable
            ? "Not linked to an AI task"
            : chat.connected
              ? chat.agentLabel
              : "Connecting…"
        }
        mode={mode}
        onModeChange={onModeChange}
        onSideChange={onSideChange}
        onClose={onClose}
      />

      <ChatMessageList
        messages={chat.messages}
        streaming={chat.streaming}
        agentLabel={chat.agentLabel}
        displaySettings={{ showThinking: true, showTools: true }}
        workspacePath={preview.workspace}
      />
      <PendingPermissions
        permissions={chat.pendingPermissions}
        onRespond={chat.sendPermissionResponse}
        onCancel={chat.cancelPermissionRequest}
      />
      <ChatInput
        value={input}
        onChange={(value) => {
          setInput(value);
          setSubmitError("");
        }}
        onSubmit={submit}
        disabled={!preview.chatAvailable}
        submitDisabled={!chat.connected || chat.streaming}
        isStreaming={chat.streaming}
        onStop={chat.stopStreaming}
        showCommands={false}
        contextCanSubmit={drafts.length > 0}
        contextContent={
          drafts.length > 0 || submitError ? (
            <div className="space-y-1.5 px-3 pt-3">
              <div className="flex flex-wrap gap-1.5">
                {drafts.map((draft) => (
                  <span
                    key={draft.id}
                    className="flex max-w-full items-center rounded-full border border-border bg-background text-xs text-muted-foreground"
                  >
                    <button
                      type="button"
                      className="flex min-w-0 items-center gap-1.5 py-1 pl-2.5 pr-1 hover:text-foreground"
                      onClick={() => onFocusDraft(draft.id)}
                      title={draft.comment}
                    >
                      <MessageSquare className="h-3 w-3 shrink-0 text-primary" />
                      <span className="truncate">{draftExcerpt(draft)}</span>
                    </button>
                    <button
                      type="button"
                      className="mr-1 rounded-full p-1 hover:text-foreground"
                      onClick={() => onRemoveDraft(draft.id)}
                      aria-label={`Remove comment on ${draftExcerpt(draft)}`}
                    >
                      <X className="h-3 w-3" />
                    </button>
                  </span>
                ))}
              </div>
              {submitError && (
                <p className="text-xs text-destructive">{submitError}</p>
              )}
            </div>
          ) : undefined
        }
        leadingAction={
          supportsElementSelection ? (
            <Button
              type="button"
              variant={elementSelectionActive ? "secondary" : "ghost"}
              size="icon-sm"
              onClick={onSelectElement}
              aria-label="Select element"
              aria-pressed={elementSelectionActive}
              title="Select element"
              className="h-8 w-8 text-muted-foreground"
            >
              <MousePointer2 className="h-4 w-4" />
            </Button>
          ) : undefined
        }
        placeholder="Ask for a change…"
        targetLabel={
          !preview.chatAvailable
            ? "Preview chat unavailable"
            : chat.connected
              ? chat.agentLabel
              : "Connecting…"
        }
      />
    </aside>
  );
}
