import { expect, test } from "bun:test";
import type { SessionNotification } from "@agentclientprotocol/sdk";

import {
  applyPreviewSessionNotification,
  previewSessionChanged,
} from "../src/preview/previewChatMessages";
import {
  isPreviewRefreshEvent,
  previewConversationThreadId,
  previewSocketUrl,
} from "../src/preview/usePreviewChatConnection";
import {
  buildPreviewReviewPrompt,
  previewReviewDisplay,
} from "../src/preview/previewReview";
import { ownerPreviewSlug } from "../src/preview/previewRoute";
import {
  parsePreviewBootstrap,
  type PreviewItem,
  type PreviewReviewDraft,
} from "../src/preview/previewTypes";

const preview: PreviewItem = {
  slug: "readme-cn-md",
  title: "README 中文版",
  workspace: "/work/VibeAround",
  kind: "file",
  src: "/va/preview/u/readme-cn-md/content",
  chatAvailable: true,
};

test("owner Preview route matches only the page shell", () => {
  expect(ownerPreviewSlug("/va/preview/u/readme-cn-md")).toBe("readme-cn-md");
  expect(ownerPreviewSlug("/va/preview/u/readme%20cn")).toBe("readme cn");
  expect(ownerPreviewSlug("/va/preview/u/readme/content")).toBeNull();
  expect(ownerPreviewSlug("/va/preview/s/readme")).toBeNull();
  expect(ownerPreviewSlug("/va/")).toBeNull();
});

test("Preview bootstrap rejects partial item contracts", () => {
  expect(
    parsePreviewBootstrap({ selectedSlug: preview.slug, previews: [preview] }),
  ).toEqual({ selectedSlug: preview.slug, previews: [preview] });
  expect(
    parsePreviewBootstrap({
      selectedSlug: preview.slug,
      previews: [{ ...preview, chatAvailable: "yes" }],
    }),
  ).toBeNull();
});

test("review submission carries source location while the visible message stays readable", () => {
  const drafts: PreviewReviewDraft[] = [
    {
      id: "review-1",
      anchor: {
        kind: "text",
        text: "Host-side Web Search",
        heading: "Web Search",
        startLine: 128,
        endLine: 129,
      },
      comment: "翻译成中文",
    },
  ];
  const prompt = buildPreviewReviewPrompt(preview, drafts, "顺便检查语气");
  const display = previewReviewDisplay(drafts, "顺便检查语气");

  expect(prompt).toContain("Source lines: 128-129");
  expect(prompt).toContain("Section: Web Search");
  expect(prompt).toContain("--- BEGIN QUOTED PREVIEW CONTENT ---");
  expect(prompt).toContain("翻译成中文");
  expect(display).toContain("lines 128–129 · Web Search");
  expect(display).toContain("“Host-side Web Search”");
  expect(display).toContain("→ 翻译成中文");
  expect(display).not.toContain("BEGIN QUOTED");
});

test("echoed hidden review prompt acknowledges the optimistic visible summary", () => {
  const current = [
    {
      role: "user" as const,
      content: "Web Search\n“Host-side Web Search”\n→ 翻译成中文",
      messageId: "message-1",
      optimistic: true,
    },
  ];
  const notification = {
    sessionId: "session-1",
    update: {
      sessionUpdate: "user_message_chunk",
      messageId: "message-1",
      content: {
        type: "text",
        text: "Please update this Preview using the review notes below…",
      },
    },
  } as SessionNotification;

  const next = applyPreviewSessionNotification(current, notification);
  expect(next).toHaveLength(1);
  expect(next[0].content).toContain("→ 翻译成中文");
  expect(next[0].content).not.toContain("Please update this Preview");
  expect(next[0].optimistic).toBe(false);
});

test("Preview transcript resets only when the bound agent session changes", () => {
  expect(previewSessionChanged(null, "session-1")).toBe(false);
  expect(previewSessionChanged("session-1", "session-1")).toBe(false);
  expect(previewSessionChanged("session-1", "session-2")).toBe(true);
});

test("Preview refresh is a local websocket frame outside the shared chat schema", () => {
  expect(isPreviewRefreshEvent({ kind: "preview_refresh" })).toBe(true);
  expect(isPreviewRefreshEvent({ kind: "turn_status", active: false })).toBe(false);
  expect(isPreviewRefreshEvent("preview_refresh")).toBe(false);
});

test("Preview conversation accepts only a non-empty thread id", () => {
  expect(
    previewConversationThreadId({
      kind: "preview_conversation",
      thread_id: " wt_review ",
    }),
  ).toBe("wt_review");
  expect(
    previewConversationThreadId({
      kind: "preview_conversation",
      thread_id: "   ",
    }),
  ).toBeNull();
  expect(
    previewConversationThreadId({ kind: "preview_conversation" }),
  ).toBeNull();
  expect(
    previewConversationThreadId({ kind: "session_ready", thread_id: "wt_other" }),
  ).toBeNull();
});

test("Preview websocket carries the saved conversation thread as a hint", () => {
  expect(
    previewSocketUrl(
      "readme cn",
      "wt_review/1",
      "https://va.example/va/preview/u/readme-cn",
    ),
  ).toBe(
    "wss://va.example/va/preview/u/readme%20cn/chat?thread_id=wt_review%2F1",
  );
  expect(
    previewSocketUrl(
      "readme-cn",
      null,
      "http://127.0.0.1:12358/va/preview/u/readme-cn",
    ),
  ).toBe("ws://127.0.0.1:12358/va/preview/u/readme-cn/chat");
});
