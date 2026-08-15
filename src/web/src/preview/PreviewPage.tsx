import { useCallback, useEffect, useRef, useState } from "react";
import { MousePointer2, ScanLine, X } from "lucide-react";

import { Button } from "@/components/ui/button";
import { ReviewPopover } from "@/components/review/ReviewPopover";
import type {
  ReviewTool,
  ReviewToolbarModel,
} from "@/components/review/reviewTypes";
import { useReviewBridge } from "@/components/review/useReviewBridge";
import { PreviewChatDrawer } from "./PreviewChatDrawer";
import type { PreviewChatMode, PreviewChatSide } from "./previewChatLayout";
import {
  PreviewHelper,
  type PreviewHelperCorner,
  type PreviewHelperView,
} from "./PreviewHelper";
import {
  parsePreviewBootstrap,
  refreshedPreviewSlug,
  type PreviewBootstrap,
} from "./previewTypes";
import { usePreviewChatConnection } from "./usePreviewChatConnection";
import {
  rememberServerPreviewConsent,
  serverPreviewNeedsConsent,
} from "./previewConsent";

export default function PreviewPage({ initialSlug }: { initialSlug: string }) {
  const [bootstrap, setBootstrap] = useState<PreviewBootstrap>();
  const [error, setError] = useState<string>();

  useEffect(() => {
    const abort = new AbortController();
    setBootstrap(undefined);
    setError(undefined);
    void fetchPreviewBootstrap(initialSlug, abort.signal)
      .then((parsed) => {
        if (!parsed.previews.some((preview) => preview.slug === initialSlug)) {
          throw new Error("This Preview is no longer active.");
        }
        setBootstrap(parsed);
      })
      .catch((reason: unknown) => {
        if (abort.signal.aborted) return;
        setError(reason instanceof Error ? reason.message : "Preview failed to load.");
      });
    return () => abort.abort();
  }, [initialSlug]);

  const refreshBootstrap = useCallback(async (slug: string) => {
    try {
      const next = await fetchPreviewBootstrap(slug);
      setBootstrap(next);
      setError(undefined);
      return next;
    } catch (reason: unknown) {
      setError(
        reason instanceof Error ? reason.message : "Preview failed to load.",
      );
      return null;
    }
  }, []);

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

  return (
    <PreviewWorkspace
      bootstrap={bootstrap}
      requestedSlug={initialSlug}
      onRefreshBootstrap={refreshBootstrap}
    />
  );
}

async function fetchPreviewBootstrap(
  slug: string,
  signal?: AbortSignal,
): Promise<PreviewBootstrap> {
  const response = await fetch(
    `/va/preview/u/${encodeURIComponent(slug)}/bootstrap`,
    { signal, credentials: "same-origin" },
  );
  if (!response.ok) {
    const detail = (await response.text()).trim();
    throw new Error(detail || `Preview failed to load (${response.status})`);
  }
  const parsed = parsePreviewBootstrap(await response.json());
  if (!parsed || parsed.previews.length === 0) {
    throw new Error("Preview response is invalid.");
  }
  return parsed;
}

function PreviewWorkspace({
  bootstrap,
  requestedSlug,
  onRefreshBootstrap,
}: {
  bootstrap: PreviewBootstrap;
  requestedSlug: string;
  onRefreshBootstrap: (slug: string) => Promise<PreviewBootstrap | null>;
}) {
  const [selectedSlug, setSelectedSlug] = useState(requestedSlug);
  const [helperView, setHelperView] =
    useState<PreviewHelperView>("collapsed");
  const [helperCorner, setHelperCorner] =
    useState<PreviewHelperCorner>("bottom-right");
  const [chatMode, setChatMode] = useState<PreviewChatMode>("impact");
  const [chatWidth, setChatWidth] = useState(448);
  const [frameRevision, setFrameRevision] = useState(0);
  const [consentRevision, setConsentRevision] = useState(0);
  const frameRef = useRef<HTMLIFrameElement>(null);
  const selected =
    bootstrap.previews.find((preview) => preview.slug === selectedSlug) ??
    bootstrap.previews[0];

  const review = useReviewBridge(frameRef, {
    id: selected.slug,
    src: selected.src,
  });
  const needsServerConsent = serverPreviewNeedsConsent(
    selected,
    window.sessionStorage,
  );

  const reloadPreview = useCallback(async () => {
    const nextBootstrap = await onRefreshBootstrap(selected.slug);
    if (!nextBootstrap) return;
    const nextSlug = refreshedPreviewSlug(nextBootstrap, selected.slug);
    if (!nextSlug) return;

    review.prepareFrame();
    setSelectedSlug(nextSlug);
    setFrameRevision((revision) => revision + 1);
    if (nextSlug !== selected.slug) {
      history.replaceState(
        null,
        "",
        `/va/preview/u/${encodeURIComponent(nextSlug)}`,
      );
    }
  }, [onRefreshBootstrap, review.prepareFrame, selected.slug]);
  const refreshPreview = useCallback(async () => {
    const hasReviewDraft = review.drafts.length > 0 || review.editor !== null;
    if (
      hasReviewDraft &&
      !window.confirm(
        "Refreshing this Preview will clear all review drafts. Continue?",
      )
    ) {
      return;
    }
    await reloadPreview();
  }, [reloadPreview, review.drafts.length, review.editor]);
  const chat = usePreviewChatConnection(selected.slug, reloadPreview);

  useEffect(() => {
    document.title = `Preview — ${selected.title}`;
  }, [selected.title]);

  useEffect(() => {
    if (!review.pickMode) return;
    const cancel = (event: KeyboardEvent) => {
      if (event.key === "Escape") review.setPickMode(null);
    };
    document.addEventListener("keydown", cancel, true);
    return () => document.removeEventListener("keydown", cancel, true);
  }, [review.pickMode, review.setPickMode]);

  const selectPreview = (slug: string) => {
    if (slug === selected.slug) return;
    review.prepareFrame();
    setSelectedSlug(slug);
    setFrameRevision((revision) => revision + 1);
    history.replaceState(null, "", `/va/preview/u/${encodeURIComponent(slug)}`);
  };

  const chatSide: PreviewChatSide = helperCorner.endsWith("left")
    ? "left"
    : "right";

  const changeChatSide = (side: PreviewChatSide) => {
    setHelperCorner((corner) => {
      const vertical = corner.startsWith("top") ? "top" : "bottom";
      return `${vertical}-${side}`;
    });
  };

  const revealPreviewOnCompactScreen = () => {
    if (window.matchMedia("(max-width: 1023px)").matches) {
      setHelperView("collapsed");
    }
  };

  const activeDraft = review.editor?.draftId
    ? review.drafts.find((draft) => draft.id === review.editor?.draftId)
    : undefined;

  const activeReviewTool: ReviewTool | null = review.pickMode;
  const reviewToolbar: ReviewToolbarModel = {
    activeTool: activeReviewTool,
    elementAvailable: review.capabilities.includes("element"),
    regionAvailable: review.capabilities.includes("region"),
    textSelectionAvailable: review.capabilities.includes("text"),
    onToolChange: (tool) => {
      revealPreviewOnCompactScreen();
      review.setPickMode(tool);
    },
  };

  return (
    <div className="relative flex h-full min-h-0 overflow-hidden bg-muted/20">
      <div className="relative order-1 min-w-0 flex-1">
        {needsServerConsent ? (
          <div className="flex h-full items-center justify-center bg-muted/20 p-6">
            <section className="max-w-lg rounded-xl border border-border bg-background p-6 shadow-sm">
              <h1 className="text-lg font-semibold">Open this Server Preview?</h1>
              <p className="mt-2 text-sm leading-6 text-muted-foreground">
                This loads content from a local development server. Continue only
                if you trust the project and its dependencies; the page can behave
                like any site you open in your browser.
              </p>
              <Button
                className="mt-5"
                onClick={() => {
                  rememberServerPreviewConsent(selected, window.sessionStorage);
                  setConsentRevision((revision) => revision + 1);
                }}
              >
                Continue to Preview
              </Button>
            </section>
          </div>
        ) : (
          <iframe
            key={`${selected.slug}:${frameRevision}:${consentRevision}`}
            ref={frameRef}
            src={selected.src}
            title={`Preview content — ${selected.title}`}
            referrerPolicy="no-referrer"
            onLoad={review.handleFrameLoad}
            className="h-full w-full border-0 bg-white"
          />
        )}

        <PreviewHelper
          view={helperView}
          corner={helperCorner}
          previews={bootstrap.previews}
          selected={selected}
          onViewChange={setHelperView}
          onCornerChange={setHelperCorner}
          onSelectPreview={selectPreview}
          onRefresh={refreshPreview}
          reviewToolbar={reviewToolbar}
        />
      </div>

      {review.pickMode && (
        <button
          type="button"
          className="fixed bottom-5 left-1/2 z-30 flex -translate-x-1/2 items-center gap-2 rounded-full border border-primary/30 bg-background px-3 py-2 text-xs font-medium text-primary shadow-lg"
          onClick={() => review.setPickMode(null)}
        >
          {review.pickMode === "element" ? (
            <MousePointer2 className="h-3.5 w-3.5" />
          ) : (
            <ScanLine className="h-3.5 w-3.5" />
          )}
          {review.pickMode === "element"
            ? "Click an element to comment"
            : "Drag over a region to capture"}
          <span className="text-muted-foreground">· Esc to cancel</span>
        </button>
      )}

      {review.captureError && (
        <button
          type="button"
          className="fixed bottom-5 left-1/2 z-50 flex max-w-[calc(100vw-2rem)] -translate-x-1/2 items-center gap-2 rounded-lg border border-destructive/30 bg-background px-3 py-2 text-xs text-destructive shadow-lg"
          onClick={review.clearCaptureError}
        >
          <span className="truncate">{review.captureError}</span>
          <X className="h-3.5 w-3.5 shrink-0" />
        </button>
      )}

      <PreviewChatDrawer
        open={helperView === "chat"}
        previews={bootstrap.previews}
        preview={selected}
        chat={chat}
        drafts={review.drafts}
        reviewToolbar={reviewToolbar}
        mode={chatMode}
        side={chatSide}
        width={chatWidth}
        onClose={() => setHelperView("expanded")}
        onSelectPreview={selectPreview}
        onRefresh={refreshPreview}
        onModeChange={setChatMode}
        onSideChange={changeChatSide}
        onWidthChange={setChatWidth}
        onFocusDraft={(id) => {
          revealPreviewOnCompactScreen();
          review.focusDraft(id);
        }}
        onRemoveDraft={review.removeDraft}
        onClearSubmittedDrafts={review.clearSubmittedDrafts}
      />

      {review.editor && (
        <ReviewPopover
          editor={review.editor}
          frameRef={frameRef}
          initialComment={activeDraft?.comment ?? ""}
          onSave={(comment) => {
            if (review.saveEditor(comment)) setHelperView("chat");
          }}
          onCancel={() => review.closeEditor(true)}
        />
      )}
    </div>
  );
}
