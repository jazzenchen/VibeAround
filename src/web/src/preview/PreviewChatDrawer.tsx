import { useEffect, useState } from "react";
import { MessageSquare, MousePointer2, RotateCw, X } from "lucide-react";

import {
  ChatInput,
  ChatMessageList,
  PendingPermissions,
} from "@/components/chat/chatUi";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
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
  refreshAvailable: boolean;
  onOpenChange: (open: boolean) => void;
  onRefresh: () => void;
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
  refreshAvailable,
  onOpenChange,
  onRefresh,
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

  return (
    <>
      <Button
        type="button"
        size="sm"
        onClick={() => onOpenChange(!open)}
        className={cn(
          "fixed bottom-5 right-5 z-30 rounded-full shadow-lg transition-transform",
          open && "hidden sm:inline-flex sm:right-[29rem]",
        )}
        aria-label="Preview conversation"
        aria-expanded={open}
      >
        <MessageSquare className="h-4 w-4" />
        <span>Chat</span>
        {drafts.length > 0 && (
          <span className="rounded-full bg-primary-foreground/20 px-1.5 text-[10px]">
            {drafts.length}
          </span>
        )}
      </Button>

      <aside
        aria-label="Preview conversation"
        aria-hidden={!open}
        inert={!open}
        className={cn(
          "fixed inset-y-0 right-0 z-40 flex w-full flex-col border-l border-border bg-background shadow-2xl transition-transform duration-200 sm:w-[28rem]",
          open ? "translate-x-0" : "translate-x-full",
        )}
      >
        <header className="flex h-14 shrink-0 items-center gap-2 border-b border-border px-3">
          <div className="min-w-0 flex-1">
            <div className="truncate text-sm font-semibold">Preview conversation</div>
            <div className="truncate text-xs text-muted-foreground">
              {!preview.chatAvailable
                ? "Not linked to an AI task"
                : chat.connected
                  ? chat.agentLabel
                  : "Connecting…"}
            </div>
          </div>
          {refreshAvailable && (
            <Button type="button" variant="outline" size="sm" onClick={onRefresh}>
              <RotateCw className="h-3.5 w-3.5" />
              Refresh preview
            </Button>
          )}
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            onClick={() => onOpenChange(false)}
            aria-label="Close conversation"
          >
            <X className="h-4 w-4" />
          </Button>
        </header>

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
                variant="ghost"
                size="icon-sm"
                onClick={onSelectElement}
                aria-label="Select element"
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
    </>
  );
}
