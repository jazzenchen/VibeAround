import { useCallback, useEffect, useRef, useState } from "react";
import { MousePointer2, RefreshCw } from "lucide-react";

import { Button } from "@/components/ui/button";
import { PreviewChatDrawer } from "./PreviewChatDrawer";
import { PreviewPicker } from "./PreviewPicker";
import { PreviewReviewPopover } from "./PreviewReviewPopover";
import { parsePreviewBootstrap, type PreviewBootstrap } from "./previewTypes";
import { usePreviewChatConnection } from "./usePreviewChatConnection";
import { usePreviewReviewBridge } from "./usePreviewReviewBridge";

export default function PreviewPage({ initialSlug }: { initialSlug: string }) {
  const [bootstrap, setBootstrap] = useState<PreviewBootstrap>();
  const [error, setError] = useState<string>();

  useEffect(() => {
    const abort = new AbortController();
    setBootstrap(undefined);
    setError(undefined);
    void fetch(
      `/va/preview/u/${encodeURIComponent(initialSlug)}/bootstrap`,
      { signal: abort.signal, credentials: "same-origin" },
    )
      .then(async (response) => {
        if (!response.ok) {
          const detail = (await response.text()).trim();
          throw new Error(detail || `Preview failed to load (${response.status})`);
        }
        const parsed = parsePreviewBootstrap(await response.json());
        if (!parsed || parsed.previews.length === 0) {
          throw new Error("Preview response is invalid.");
        }
        setBootstrap(parsed);
      })
      .catch((reason: unknown) => {
        if (abort.signal.aborted) return;
        setError(reason instanceof Error ? reason.message : "Preview failed to load.");
      });
    return () => abort.abort();
  }, [initialSlug]);

  if (error) {
    return (
      <main className="flex h-full items-center justify-center bg-muted/20 p-6">
        <section className="max-w-md rounded-xl border border-border bg-background p-6 text-center shadow-sm">
          <img
            src="/va/brand/vibearound-mark.svg"
            alt=""
            className="mx-auto h-10 w-10"
          />
          <h1 className="mt-3 text-lg font-semibold">Preview unavailable</h1>
          <p className="mt-1 text-sm text-muted-foreground">{error}</p>
        </section>
      </main>
    );
  }

  if (!bootstrap) {
    return (
      <main className="flex h-full items-center justify-center bg-muted/20">
        <p className="animate-pulse text-sm text-muted-foreground">Loading Preview…</p>
      </main>
    );
  }

  return <PreviewWorkspace bootstrap={bootstrap} requestedSlug={initialSlug} />;
}

function PreviewWorkspace({
  bootstrap,
  requestedSlug,
}: {
  bootstrap: PreviewBootstrap;
  requestedSlug: string;
}) {
  const initialSelectedSlug =
    [bootstrap.selectedSlug, requestedSlug].find((slug) =>
      bootstrap.previews.some((preview) => preview.slug === slug),
    ) ?? bootstrap.previews[0].slug;
  const [selectedSlug, setSelectedSlug] = useState(initialSelectedSlug);
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [frameRevision, setFrameRevision] = useState(0);
  const [lastRefreshedAt, setLastRefreshedAt] = useState(Date.now());
  const frameRef = useRef<HTMLIFrameElement>(null);
  const selected =
    bootstrap.previews.find((preview) => preview.slug === selectedSlug) ??
    bootstrap.previews[0];

  const chat = usePreviewChatConnection(selected.slug, selected.chatAvailable);
  const review = usePreviewReviewBridge(frameRef, selected);

  const refreshPreview = useCallback(() => {
    review.prepareFrame();
    setFrameRevision((revision) => revision + 1);
    setLastRefreshedAt(Date.now());
  }, [review.prepareFrame]);

  useEffect(() => {
    if (chat.refreshRequestVersion > 0) refreshPreview();
  }, [chat.refreshRequestVersion, refreshPreview]);

  useEffect(() => {
    document.title = `Preview — ${selected.title}`;
  }, [selected.title]);

  useEffect(() => {
    if (!review.elementMode) return;
    const cancel = (event: KeyboardEvent) => {
      if (event.key === "Escape") review.setElementMode(false);
    };
    document.addEventListener("keydown", cancel, true);
    return () => document.removeEventListener("keydown", cancel, true);
  }, [review.elementMode, review.setElementMode]);

  const selectPreview = (slug: string) => {
    if (slug === selected.slug) return;
    review.prepareFrame();
    setSelectedSlug(slug);
    setFrameRevision((revision) => revision + 1);
    setLastRefreshedAt(Date.now());
    history.replaceState(null, "", `/va/preview/u/${encodeURIComponent(slug)}`);
  };

  const activeDraft = review.editor?.draftId
    ? review.drafts.find((draft) => draft.id === review.editor?.draftId)
    : undefined;

  return (
    <div className="relative flex h-full min-h-0 flex-col overflow-hidden bg-muted/20">
      <header className="z-20 flex shrink-0 items-center gap-3 border-b border-border bg-background/95 px-3 py-2 shadow-sm backdrop-blur sm:px-4">
        <div className="hidden shrink-0 items-center gap-2 sm:flex">
          <img src="/va/brand/vibearound-mark.svg" alt="" className="h-7 w-7" />
          <span className="text-sm font-semibold">VibeAround Preview</span>
        </div>
        <PreviewPicker
          previews={bootstrap.previews}
          selected={selected}
          onSelect={selectPreview}
        />
        <Button
          type="button"
          variant="outline"
          size="icon"
          onClick={refreshPreview}
          aria-label="Refresh preview"
          title="Refresh preview"
          className="shadow-none"
        >
          <RefreshCw className="h-4 w-4" />
        </Button>
      </header>

      <iframe
        key={`${selected.slug}:${frameRevision}`}
        ref={frameRef}
        src={selected.src}
        title={`Preview content — ${selected.title}`}
        referrerPolicy="no-referrer"
        onLoad={review.handleFrameLoad}
        className="min-h-0 flex-1 border-0 bg-white"
      />

      {review.elementMode && (
        <button
          type="button"
          className="fixed bottom-5 left-1/2 z-30 flex -translate-x-1/2 items-center gap-2 rounded-full border border-primary/30 bg-background px-3 py-2 text-xs font-medium text-primary shadow-lg"
          onClick={() => review.setElementMode(false)}
        >
          <MousePointer2 className="h-3.5 w-3.5" />
          Click an element to comment · Esc to cancel
        </button>
      )}

      <PreviewChatDrawer
        open={drawerOpen}
        preview={selected}
        chat={chat}
        drafts={review.drafts}
        supportsElementSelection={review.capabilities.includes("element")}
        refreshAvailable={
          chat.lastTurnCompletedAt !== undefined &&
          chat.lastTurnCompletedAt > lastRefreshedAt
        }
        onOpenChange={setDrawerOpen}
        onRefresh={refreshPreview}
        onSelectElement={() => {
          setDrawerOpen(false);
          review.setElementMode(true);
        }}
        onFocusDraft={(id) => {
          setDrawerOpen(false);
          review.focusDraft(id);
        }}
        onRemoveDraft={review.removeDraft}
        onClearSubmittedDrafts={review.clearSubmittedDrafts}
      />

      {review.editor && (
        <PreviewReviewPopover
          editor={review.editor}
          frameRef={frameRef}
          initialComment={activeDraft?.comment ?? ""}
          onSave={review.saveEditor}
          onCancel={() => review.closeEditor(true)}
        />
      )}
    </div>
  );
}
