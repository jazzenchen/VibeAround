import { useEffect, useRef, useState, type CSSProperties } from "react";
import { Image, MessageSquare, X } from "lucide-react";

import {
  ChatInput,
  ChatMessageList,
  PendingPermissions,
} from "@/components/chat/chatUi";
import type { ChatAttachment } from "@/components/chat/chatTypes";
import { uploadPreviewChatFile } from "@/api/sessions";
import { cn } from "@/lib/utils";
import { PreviewChatHeader } from "./PreviewChatHeader";
import { PreviewChatResizeHandle } from "./PreviewChatResizeHandle";
import {
  PreviewReviewToolbar,
  type PreviewReviewToolbarModel,
} from "./PreviewReviewToolbar";
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
  previews: PreviewItem[];
  preview: PreviewItem;
  chat: PreviewChat;
  drafts: PreviewReviewDraft[];
  reviewToolbar: PreviewReviewToolbarModel;
  mode: PreviewChatMode;
  side: PreviewChatSide;
  width: number;
  onClose: () => void;
  onSelectPreview: (slug: string) => void;
  onRefresh: () => void;
  onModeChange: (mode: PreviewChatMode) => void;
  onSideChange: (side: PreviewChatSide) => void;
  onWidthChange: (width: number) => void;
  onFocusDraft: (id: string) => void;
  onRemoveDraft: (id: string) => void;
  onClearSubmittedDrafts: (submittedIds: string[]) => void;
};

function draftExcerpt(draft: PreviewReviewDraft) {
  const value = draft.anchor.text.replace(/\s+/g, " ").trim();
  return value.length > 38 ? `${value.slice(0, 37)}…` : value || "Selected element";
}

async function uploadReviewScreenshots(
  previewSlug: string,
  drafts: PreviewReviewDraft[],
): Promise<ChatAttachment[]> {
  const screenshots = drafts.flatMap((draft) =>
    draft.screenshot ? [draft.screenshot] : [],
  );
  return Promise.all(
    screenshots.map(async (screenshot) => {
      const uploaded = await uploadPreviewChatFile(
        previewSlug,
        new File([screenshot.blob], screenshot.fileName, {
          type: screenshot.blob.type,
        }),
      );
      return {
        id: uploaded.id,
        name: uploaded.name,
        mimeType: uploaded.mime_type,
        size: uploaded.size,
        uri: uploaded.uri,
      };
    }),
  );
}

export function PreviewChatDrawer({
  open,
  previews,
  preview,
  chat,
  drafts,
  reviewToolbar,
  mode,
  side,
  width,
  onClose,
  onSelectPreview,
  onRefresh,
  onModeChange,
  onSideChange,
  onWidthChange,
  onFocusDraft,
  onRemoveDraft,
  onClearSubmittedDrafts,
}: PreviewChatDrawerProps) {
  const [input, setInput] = useState("");
  const [submitError, setSubmitError] = useState("");
  const [uploadingScreenshots, setUploadingScreenshots] = useState(false);
  const previewSlugRef = useRef(preview.slug);
  const draftsRef = useRef(drafts);
  const submittingRef = useRef(false);
  previewSlugRef.current = preview.slug;
  draftsRef.current = drafts;

  useEffect(() => {
    setInput("");
    setSubmitError("");
  }, [preview.slug]);

  const submit = async () => {
    const prompt = input.trim();
    if ((!prompt && drafts.length === 0) || submittingRef.current) return;
    const submittedSlug = preview.slug;
    const submittedDrafts = [...drafts];
    const submittedIds = submittedDrafts.map((draft) => draft.id);
    const message = submittedDrafts.length
      ? buildPreviewReviewPrompt(preview, submittedDrafts, prompt)
      : prompt;
    const display = submittedDrafts.length
      ? previewReviewDisplay(submittedDrafts, prompt)
      : prompt;
    if (message.length > MAX_PREVIEW_MESSAGE_LENGTH) {
      setSubmitError("These review notes are too long for one message.");
      return;
    }
    const hasScreenshots = submittedDrafts.some((draft) => draft.screenshot);
    submittingRef.current = true;
    setUploadingScreenshots(hasScreenshots);
    try {
      const attachments = hasScreenshots
        ? await uploadReviewScreenshots(submittedSlug, submittedDrafts)
        : [];
      if (previewSlugRef.current !== submittedSlug) return;
      const currentDrafts = draftsRef.current;
      const unchanged = submittedDrafts.every((submitted) =>
        currentDrafts.some((current) => current === submitted),
      );
      if (!unchanged) {
        setSubmitError("Review notes changed while uploading. Submit again.");
        return;
      }
      if (!chat.sendMessage(message, display, attachments)) {
        setSubmitError("Wait for the current turn to finish.");
        return;
      }
      setInput("");
      setSubmitError("");
      if (submittedIds.length) onClearSubmittedDrafts(submittedIds);
    } catch (error) {
      setSubmitError(
        error instanceof Error ? error.message : "Screenshot upload failed.",
      );
    } finally {
      submittingRef.current = false;
      setUploadingScreenshots(false);
    }
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
        previews={previews}
        selected={preview}
        mode={mode}
        onSelectPreview={onSelectPreview}
        onRefresh={onRefresh}
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
      <PreviewReviewToolbar
        {...reviewToolbar}
        className="shrink-0 border-t border-border px-3 py-2"
      />
      <ChatInput
        value={input}
        onChange={(value) => {
          setInput(value);
          setSubmitError("");
        }}
        onSubmit={() => void submit()}
        disabled={!preview.chatAvailable}
        submitDisabled={!chat.connected || chat.streaming || uploadingScreenshots}
        isStreaming={chat.streaming}
        attachmentsUploading={uploadingScreenshots}
        attachmentsUploadingCount={drafts.filter((draft) => draft.screenshot).length}
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
                      {draft.screenshot ? (
                        <Image className="h-3 w-3 shrink-0 text-primary" />
                      ) : (
                        <MessageSquare className="h-3 w-3 shrink-0 text-primary" />
                      )}
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
