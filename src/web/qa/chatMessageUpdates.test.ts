import { expect, test } from "bun:test";

import {
  appendStreamAssistantMessage,
  appendUserMessageChunk,
} from "../src/components/chat/chatMessageUpdates";
import type { ChatMessage } from "../src/components/chat/chatTypes";

test("assistant reply stays after consecutive user messages", () => {
  const messages: ChatMessage[] = [
    {
      role: "user",
      content: "first",
      messageId: "user-1",
      optimistic: true,
    },
    {
      role: "user",
      content: "second",
      messageId: "user-2",
      optimistic: true,
    },
    {
      role: "user",
      content: "third",
      messageId: "user-3",
      optimistic: true,
    },
  ];

  const confirmed = appendUserMessageChunk(
    messages,
    { type: "text", text: "first" },
    "user-1",
  );
  const withReply = appendStreamAssistantMessage(
    confirmed,
    { type: "text", text: "reply" },
    "assistant-1",
  );

  expect(withReply.map((message) => message.content)).toEqual([
    "first",
    "second",
    "third",
    "reply",
  ]);
  expect(withReply[0]?.optimistic).toBe(false);
});

test("active reply continues after a new user message without moving earlier text", () => {
  const messages: ChatMessage[] = [
    { role: "user", content: "first", messageId: "user-1" },
    {
      role: "assistant",
      content: "partial ",
      messageId: "assistant-1",
      mode: "stream",
    },
  ];

  const withQuestion = appendUserMessageChunk(
    messages,
    { type: "text", text: "follow-up" },
    "user-2",
  );
  const continuedReply = appendStreamAssistantMessage(
    withQuestion,
    { type: "text", text: "reply" },
    "assistant-1",
  );

  expect(continuedReply.map((message) => message.content)).toEqual([
    "first",
    "partial ",
    "follow-up",
    "reply",
  ]);
});
