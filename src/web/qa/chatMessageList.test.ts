import { expect, test } from "bun:test";

import { shouldShowWorkingIndicator } from "../src/components/chat/ChatMessageList";
import type { ChatDisplaySettings, ChatMessage } from "../src/components/chat/chatTypes";

const hiddenTools: ChatDisplaySettings = { showThinking: true, showTools: false };
const shownTools: ChatDisplaySettings = { showThinking: true, showTools: true };
const hiddenThinking: ChatDisplaySettings = { showThinking: false, showTools: true };
const activeToolMessage: ChatMessage = {
  role: "assistant",
  content: "I will inspect this first.",
  parts: [
    {
      id: "text",
      kind: "content",
      block: { type: "text", text: "I will inspect this first." },
    },
    {
      id: "tool-1",
      kind: "tool_call",
      toolCallId: "tool-1",
      title: "Read file",
      active: true,
    },
  ],
};
const activeThoughtMessage: ChatMessage = {
  role: "assistant",
  content: "I will think this through.",
  parts: [
    {
      id: "thought-1",
      kind: "thought",
      blocks: [{ type: "text", text: "Checking the constraints." }],
      active: true,
    },
  ],
};

test("working fallback remains visible when active tool details are hidden", () => {
  expect(shouldShowWorkingIndicator([activeToolMessage], true, hiddenTools)).toBe(true);
  expect(shouldShowWorkingIndicator([activeToolMessage], true, shownTools)).toBe(false);
});

test("working fallback remains visible when active thinking is hidden", () => {
  expect(shouldShowWorkingIndicator([activeThoughtMessage], true, hiddenThinking)).toBe(true);
  expect(shouldShowWorkingIndicator([activeThoughtMessage], true, shownTools)).toBe(false);
});

test("working fallback is not shown after the turn ends", () => {
  expect(shouldShowWorkingIndicator([activeToolMessage], false, hiddenTools)).toBe(false);
});
