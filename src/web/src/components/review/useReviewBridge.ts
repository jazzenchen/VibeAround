import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type RefObject,
} from "react";

import type {
  ReviewAnchor,
  ReviewDraft,
  ReviewEditor,
  ReviewFrameRect,
  ReviewTool,
} from "./reviewTypes";

const REVIEW_SCOPE = "va-preview-review";
const REVIEW_VERSION = 1;

function makeId(prefix: string) {
  return typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

export function reviewHelloStartsNewEpoch(
  currentDocumentId: string | null,
  nextDocumentId: unknown,
) {
  return (
    currentDocumentId !== null &&
    typeof nextDocumentId === "string" &&
    nextDocumentId.length > 0 &&
    nextDocumentId !== currentDocumentId
  );
}

function isFrameRect(value: unknown): value is ReviewFrameRect {
  if (!value || typeof value !== "object") return false;
  const rect = value as Record<string, unknown>;
  return [rect.x, rect.y, rect.width, rect.height].every(
    (part) => typeof part === "number" && Number.isFinite(part),
  );
}

function isAnchor(value: unknown): value is ReviewAnchor {
  if (!value || typeof value !== "object") return false;
  const anchor = value as Record<string, unknown>;
  if (typeof anchor.text !== "string") return false;
  if (anchor.kind === "text" || anchor.kind === "element") return true;
  if (anchor.kind !== "region" || !anchor.region || typeof anchor.region !== "object") {
    return false;
  }
  const region = anchor.region as Record<string, unknown>;
  return [region.width, region.height].every(
    (size) => typeof size === "number" && Number.isInteger(size) && size > 0,
  );
}

function isScreenshot(value: unknown): value is Blob {
  return (
    value instanceof Blob &&
    value.type === "image/png" &&
    value.size > 0 &&
    value.size <= 20 * 1024 * 1024
  );
}

export function useReviewBridge(
  frameRef: RefObject<HTMLIFrameElement | null>,
  target: { id: string; src: string },
) {
  const [drafts, setDrafts] = useState<ReviewDraft[]>([]);
  const [editor, setEditor] = useState<ReviewEditor | null>(null);
  const [capabilities, setCapabilities] = useState<string[]>([]);
  const [pickMode, setPickModeState] = useState<ReviewTool | null>(null);
  const [captureError, setCaptureError] = useState("");
  const channelIdRef = useRef(makeId("preview-channel"));
  const documentIdRef = useRef<string | null>(null);
  const targetIdRef = useRef(target.id);
  const readyRef = useRef(false);
  const draftsRef = useRef<ReviewDraft[]>([]);

  useEffect(() => {
    draftsRef.current = drafts;
  }, [drafts]);

  const frameOrigin = useCallback(() => {
    try {
      return new URL(target.src, window.location.href).origin;
    } catch {
      return null;
    }
  }, [target.src]);

  const post = useCallback(
    (type: string, fields: Record<string, unknown> = {}) => {
      const frame = frameRef.current;
      const origin = frameOrigin();
      if (!frame?.contentWindow || !origin) return false;
      frame.contentWindow.postMessage(
        {
          scope: REVIEW_SCOPE,
          version: REVIEW_VERSION,
          channelId: channelIdRef.current,
          type,
          ...fields,
        },
        origin,
      );
      return true;
    },
    [frameOrigin, frameRef],
  );

  const command = useCallback(
    (type: string, fields?: Record<string, unknown>) =>
      readyRef.current && post(type, fields),
    [post],
  );

  const resetTransport = useCallback(() => {
    readyRef.current = false;
    setCapabilities([]);
    setEditor(null);
    setPickModeState(null);
    setCaptureError("");
  }, []);

  const rotateChannel = useCallback(() => {
    channelIdRef.current = makeId("preview-channel");
    resetTransport();
  }, [resetTransport]);

  const discardDrafts = useCallback(() => {
    draftsRef.current = [];
    setDrafts([]);
  }, []);

  const prepareFrame = useCallback(() => {
    documentIdRef.current = null;
    discardDrafts();
    rotateChannel();
  }, [discardDrafts, rotateChannel]);

  useLayoutEffect(() => {
    if (targetIdRef.current === target.id) return;
    targetIdRef.current = target.id;
    discardDrafts();
  }, [discardDrafts, target.id]);

  const handleFrameLoad = useCallback(() => {
    const documentId = documentIdRef.current;
    if (!readyRef.current && documentId) {
      post("init", { documentId });
    }
  }, [post]);

  useLayoutEffect(() => {
    const handleMessage = (event: MessageEvent) => {
      const frame = frameRef.current;
      const origin = frameOrigin();
      const message = event.data as Record<string, unknown> | null;
      if (
        !frame?.contentWindow ||
        !origin ||
        event.source !== frame.contentWindow ||
        event.origin !== origin ||
        !message ||
        message.scope !== REVIEW_SCOPE ||
        message.version !== REVIEW_VERSION
      ) {
        return;
      }

      if (message.type === "hello") {
        if (
          reviewHelloStartsNewEpoch(
            documentIdRef.current,
            message.documentId,
          )
        ) {
          rotateChannel();
        }
        if (
          typeof message.documentId !== "string" ||
          !message.documentId
        ) {
          return;
        }
        documentIdRef.current = message.documentId;
        post("init", { documentId: message.documentId });
        return;
      }
      if (message.channelId !== channelIdRef.current) return;

      if (message.type === "ready") {
        readyRef.current = true;
        setCapabilities(
          Array.isArray(message.capabilities)
            ? message.capabilities.filter(
                (capability): capability is string =>
                  typeof capability === "string",
              )
            : [],
        );
      } else if (
        message.type === "anchor-picked" &&
        typeof message.selectionId === "string" &&
        isAnchor(message.anchor) &&
        isFrameRect(message.rect)
      ) {
        const screenshot = isScreenshot(message.screenshot)
          ? {
              blob: message.screenshot,
              fileName: `preview-region-${makeId("capture")}.png`,
            }
          : undefined;
        if ((message.anchor.kind === "region") !== Boolean(screenshot)) return;
        setPickModeState(null);
        setCaptureError("");
        setEditor({
          anchorId: message.selectionId,
          selectionId: message.selectionId,
          anchor: message.anchor,
          rect: message.rect,
          screenshot,
        });
      } else if (
        message.type === "marker-activate" &&
        typeof message.markerId === "string" &&
        isFrameRect(message.rect)
      ) {
        const draft = draftsRef.current.find(
          (item) => item.id === message.markerId,
        );
        if (draft) {
          setEditor({
            anchorId: draft.id,
            draftId: draft.id,
            anchor: draft.anchor,
            rect: message.rect,
            screenshot: draft.screenshot,
          });
        }
      } else if (
        message.type === "anchor-position" &&
        typeof message.anchorId === "string"
      ) {
        setEditor((current) => {
          if (!current || current.anchorId !== message.anchorId) return current;
          return isFrameRect(message.rect)
            ? { ...current, rect: message.rect }
            : null;
        });
      } else if (message.type === "cancel") {
        setEditor(null);
        setPickModeState(null);
      } else if (
        message.type === "capture-error" &&
        typeof message.message === "string"
      ) {
        setPickModeState(null);
        setCaptureError(message.message.trim() || "Screenshot capture failed.");
      }
    };

    window.addEventListener("message", handleMessage);
    return () => window.removeEventListener("message", handleMessage);
  }, [frameOrigin, frameRef, post, rotateChannel]);

  const closeEditor = useCallback(
    (cancelNewSelection: boolean) => {
      setEditor((current) => {
        if (!current) return null;
        if (cancelNewSelection && current.selectionId && !current.draftId) {
          command("cancel", { anchorId: current.anchorId });
        } else {
          command("close-popover", { anchorId: current.anchorId });
        }
        return null;
      });
    },
    [command],
  );

  const saveEditor = useCallback(
    (comment: string) => {
      const value = comment.trim();
      if (!value || !editor) return false;
      if (editor.draftId) {
        setDrafts((current) =>
          current.map((draft) =>
            draft.id === editor.draftId ? { ...draft, comment: value } : draft,
          ),
        );
        command("close-popover", { anchorId: editor.anchorId });
      } else {
        const draft: ReviewDraft = {
          id: makeId("review"),
          anchor: editor.anchor,
          comment: value,
          screenshot: editor.screenshot,
        };
        setDrafts((current) => [...current, draft]);
        command("set-marker", {
          markerId: draft.id,
          selectionId: editor.selectionId,
        });
      }
      setEditor(null);
      return true;
    },
    [command, editor],
  );

  const removeDraft = useCallback(
    (id: string) => {
      setDrafts((current) => current.filter((draft) => draft.id !== id));
      setEditor((current) => (current?.draftId === id ? null : current));
      command("remove-marker", { markerId: id });
    },
    [command],
  );

  const focusDraft = useCallback(
    (id: string) => command("focus-marker", { markerId: id }),
    [command],
  );

  const setPickMode = useCallback(
    (mode: ReviewTool | null) => {
      const next = mode && capabilities.includes(mode) ? mode : null;
      setPickModeState(next);
      setCaptureError("");
      command("pick-mode", { mode: next ?? "none" });
    },
    [capabilities, command],
  );

  const clearSubmittedDrafts = useCallback(
    (submittedIds: string[]) => {
      const ids = new Set(submittedIds);
      for (const draft of draftsRef.current) {
        if (ids.has(draft.id)) command("remove-marker", { markerId: draft.id });
      }
      setDrafts((current) => current.filter((draft) => !ids.has(draft.id)));
      setEditor((current) =>
        current?.draftId && ids.has(current.draftId) ? null : current,
      );
    },
    [command],
  );

  return {
    drafts,
    editor,
    capabilities,
    pickMode,
    captureError,
    prepareFrame,
    handleFrameLoad,
    closeEditor,
    saveEditor,
    removeDraft,
    focusDraft,
    setPickMode,
    clearCaptureError: () => setCaptureError(""),
    clearSubmittedDrafts,
  };
}
