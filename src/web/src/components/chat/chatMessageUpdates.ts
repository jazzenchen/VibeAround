import type {
  ContentBlock,
  Plan,
  ToolCall,
  ToolCallUpdate,
} from "@agentclientprotocol/sdk";
import type {
  ChatActivity,
  ChatMessage,
  ChatMessagePart,
  ChatToolCallPart,
} from "./chatTypes";
import {
  findLastMatchingActivity,
  lastActivity,
  toolActivityId,
  toolActivityLabel,
  toolActivityStatus,
} from "./chatFrameUtils";

type ToolCallLike = ToolCall | ToolCallUpdate;

type AppendMessageOptions = {
  forceNewMessage?: boolean;
  dedupeExistingText?: boolean;
};

function partId(prefix: string) {
  return `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

function textContentBlock(text: string): ContentBlock {
  return { type: "text", text };
}

export function contentBlockText(block: ContentBlock) {
  return block.type === "text" ? block.text : "";
}

function appendTextToBlock(block: ContentBlock, text: string): ContentBlock {
  if (block.type !== "text") return block;
  return { ...block, text: `${block.text}${text}` };
}

function appendContentBlock(
  blocks: ContentBlock[] | undefined,
  block: ContentBlock,
): ContentBlock[] {
  const next = [...(blocks ?? [])];
  const last = next[next.length - 1];
  if (block.type === "text" && last?.type === "text") {
    next[next.length - 1] = appendTextToBlock(last, block.text);
    return next;
  }
  next.push(block);
  return next;
}

function appendContentPart(
  parts: ChatMessagePart[] | undefined,
  block: ContentBlock,
): ChatMessagePart[] {
  const next = [...(parts ?? [])];
  const last = next[next.length - 1];
  if (block.type === "text" && last?.kind === "content" && last.block.type === "text") {
    next[next.length - 1] = {
      ...last,
      block: appendTextToBlock(last.block, block.text),
    };
    return next;
  }
  next.push({ id: partId("content"), kind: "content", block });
  return next;
}

function withContentBlock(message: ChatMessage, block: ContentBlock): ChatMessage {
  return {
    ...message,
    content: message.content + contentBlockText(block),
    parts: appendContentPart(message.parts, block),
  };
}

function withTextPart(message: ChatMessage, text: string): ChatMessage {
  return withContentBlock(message, textContentBlock(text));
}

function stableValueKey(value: unknown): string {
  if (value === null || typeof value !== "object") {
    return JSON.stringify(value);
  }
  if (Array.isArray(value)) {
    return `[${value.map(stableValueKey).join(",")}]`;
  }
  const record = value as Record<string, unknown>;
  return `{${Object.keys(record)
    .sort()
    .map((key) => `${JSON.stringify(key)}:${stableValueKey(record[key])}`)
    .join(",")}}`;
}

function contentBlockKey(block: ContentBlock) {
  return `${block.type}:${stableValueKey(block)}`;
}

function insertMessageAt(
  messages: ChatMessage[],
  index: number,
  message: ChatMessage,
): ChatMessage[] {
  return [...messages.slice(0, index), message, ...messages.slice(index)];
}

function streamAssistantTargetIndex(messages: ChatMessage[]) {
  const tailIndex = messages.length;
  const message = messages[tailIndex - 1];
  return message?.role === "assistant" && message.mode === "stream"
    ? tailIndex - 1
    : -1;
}

function settleActiveThinking(message: ChatMessage): ChatMessage {
  const activities = message.activities?.map((activity) =>
    activity.kind === "thinking" && activity.active !== false
      ? { ...activity, active: false }
      : activity,
  );
  const parts = message.parts?.map((part) =>
    part.kind === "thought" && part.active !== false
      ? { ...part, active: false }
      : part,
  );

  if (activities === undefined && parts === undefined) return message;
  return {
    ...message,
    activities,
    parts,
    progress:
      message.progressKind === "thinking" ? undefined : message.progress,
    progressKind:
      message.progressKind === "thinking" ? undefined : message.progressKind,
  };
}

function settleActiveStreamState(message: ChatMessage): ChatMessage {
  const activities = message.activities?.map((activity) =>
    activity.active !== false ? { ...activity, active: false } : activity,
  );
  const parts = message.parts?.map((part) => {
    if (part.kind === "thought" && part.active !== false) {
      return { ...part, active: false };
    }
    if (part.kind === "tool_call" && part.active !== false) {
      return { ...part, active: false };
    }
    return part;
  });

  return {
    ...message,
    activities,
    parts,
    progress: undefined,
    progressKind: undefined,
  };
}

export function appendStandaloneAssistantMessage(
  prev: ChatMessage[],
  text: string,
): ChatMessage[] {
  if (!text) return prev;
  const next = [...prev];
  const tailIndex = next.length;
  let insertionIndex = tailIndex;
  const last = next[tailIndex - 1];
  if (
    last?.role === "assistant" &&
    last.mode === "stream" &&
    last.content === "" &&
    !last.progress &&
    !last.activities?.length &&
    !last.parts?.length
  ) {
    next.splice(tailIndex - 1, 1);
    insertionIndex -= 1;
  }
  next.splice(insertionIndex, 0, {
    role: "assistant",
    content: text,
    parts: [{ id: partId("content"), kind: "content", block: textContentBlock(text) }],
    mode: "standalone",
  });
  return next;
}

function messageIdMatches(message: ChatMessage, messageId?: string | null) {
  return messageId ? message.messageId === messageId : !message.messageId;
}

function messageHasSameBlock(
  message: ChatMessage,
  block: ContentBlock,
  options: AppendMessageOptions = {},
) {
  if (block.type === "text") {
    return options.dedupeExistingText && message.content.includes(block.text);
  }
  return message.parts?.some(
    (part) =>
      part.kind === "content" &&
      contentBlockKey(part.block) === contentBlockKey(block),
  );
}

function withMergedUserTextBlock(message: ChatMessage, text: string): ChatMessage {
  if (text.startsWith(message.content)) {
    let insertedText = false;
    const parts = (message.parts ?? []).flatMap((part): ChatMessagePart[] => {
      if (part.kind !== "content" || part.block.type !== "text") return [part];
      if (insertedText) return [];
      insertedText = true;
      return [{ ...part, block: textContentBlock(text) }];
    });
    if (!insertedText) {
      parts.unshift({ id: partId("content"), kind: "content", block: textContentBlock(text) });
    }
    return { ...message, content: text, parts };
  }
  return withTextPart(message, text);
}

function semanticPartKey(part: ChatMessagePart) {
  switch (part.kind) {
    case "content":
      return `content:${contentBlockKey(part.block)}`;
    case "plan":
      return `plan:${stableValueKey(part.plan)}`;
    case "thought":
      return `thought:${part.active === false ? "done" : "active"}:${part.blocks.map(contentBlockKey).join("|")}`;
    case "tool_call":
      return `tool:${part.toolCallId}:${part.title}:${part.status ?? ""}:${part.active === false ? "done" : "active"}:${stableValueKey(part.locations ?? null)}:${stableValueKey(part.content ?? null)}:${stableValueKey(part.rawInput ?? null)}:${stableValueKey(part.rawOutput ?? null)}`;
  }
}

function semanticActivityKey(activity: ChatActivity) {
  return `${activity.kind}:${activity.label}:${activity.detail ?? ""}:${activity.status ?? ""}:${activity.active === false ? "done" : "active"}`;
}

function semanticMessageKey(message: ChatMessage) {
  const parts = message.parts?.map(semanticPartKey).join("|") ?? "";
  const activities = message.activities?.map(semanticActivityKey).join("|") ?? "";
  return [
    message.role,
    message.content,
    message.messageId ?? "",
    message.optimistic ? "optimistic" : "",
    message.progress ?? "",
    message.progressKind ?? "",
    message.mode ?? "",
    parts,
    activities,
  ].join("\u0000");
}

function isEmptyStreamAssistant(message: ChatMessage) {
  return (
    message.role === "assistant" &&
    message.mode === "stream" &&
    message.content === "" &&
    !message.progress &&
    !message.activities?.length &&
    !message.parts?.length
  );
}

export function appendUserMessageChunk(
  prev: ChatMessage[],
  block: ContentBlock,
  messageId?: string | null,
  options: AppendMessageOptions = {},
): ChatMessage[] {
  const text = contentBlockText(block);
  if (!text && block.type === "text") return prev;
  if (messageId) {
    const existingIndex = prev.findIndex(
      (message) => message.role === "user" && message.messageId === messageId,
    );
    if (existingIndex >= 0) {
      const existing = prev[existingIndex];
      const sameBlock = messageHasSameBlock(existing, block, options);
      if (sameBlock && !existing.optimistic) return prev;
      const next = [...prev];
      const updated = sameBlock
        ? existing
        : block.type === "text"
          ? withMergedUserTextBlock(existing, block.text)
          : withContentBlock(existing, block);
      next[existingIndex] = { ...updated, optimistic: false };
      return next;
    }
  }
  const tailIndex = prev.length;
  if (prev.length === 0) {
    return [
      {
        role: "user",
        content: text,
        parts: [{ id: partId("content"), kind: "content", block }],
        messageId,
      },
    ];
  }
  const last = prev[tailIndex - 1];
  if (
    options.forceNewMessage ||
    !last ||
    last.role !== "user" ||
    !messageIdMatches(last, messageId)
  ) {
    return insertMessageAt(prev, tailIndex, {
      role: "user",
      content: text,
      parts: [{ id: partId("content"), kind: "content", block }],
      messageId,
      optimistic: false,
    });
  }
  const next = [...prev];
  next[tailIndex - 1] = {
    ...withContentBlock(last, block),
    messageId: last.messageId ?? messageId,
    optimistic: false,
  };
  return next;
}

function stableMessageKey(message: ChatMessage, index: number) {
  if (message.messageId) return `${message.role}:id:${message.messageId}`;
  const parts = message.parts?.map(semanticPartKey);
  return `${message.role}:pos:${index}:${message.content}:${parts?.join("|") ?? ""}`;
}

function sameMessageShape(a: ChatMessage, b: ChatMessage) {
  return (
    a.role === b.role &&
    a.content === b.content &&
    a.messageId === b.messageId &&
    Boolean(a.optimistic) === Boolean(b.optimistic) &&
    a.progress === b.progress &&
    a.progressKind === b.progressKind &&
    a.mode === b.mode &&
    (a.parts ?? []).map(semanticPartKey).join("|") ===
      (b.parts ?? []).map(semanticPartKey).join("|") &&
    (a.activities ?? []).map(semanticActivityKey).join("|") ===
      (b.activities ?? []).map(semanticActivityKey).join("|")
  );
}

export function mergeChatMessageSnapshots(
  current: ChatMessage[],
  incoming: ChatMessage[],
): ChatMessage[] {
  if (current.length === 0) return incoming;
  if (incoming.length === 0) return current;

  const currentByKey = new Map(
    current.map((message, index) => [
      `${stableMessageKey(message, index)}:${semanticMessageKey(message)}`,
      message,
    ]),
  );
  let changed = current.length !== incoming.length;
  const merged = incoming.map((message, index) => {
    const currentMessage = currentByKey.get(
      `${stableMessageKey(message, index)}:${semanticMessageKey(message)}`,
    );
    if (!currentMessage) {
      changed = true;
      return message;
    }
    if (sameMessageShape(currentMessage, message)) return currentMessage;
    changed = true;
    return message;
  });

  return changed ? merged : current;
}

export function appendStreamAssistantMessage(
  prev: ChatMessage[],
  block: ContentBlock,
  messageId?: string | null,
  options: AppendMessageOptions = {},
): ChatMessage[] {
  const text = contentBlockText(block);
  if (!text && block.type === "text") return prev;
  const freshMessage: ChatMessage = {
    role: "assistant",
    content: text,
    parts: [{ id: partId("content"), kind: "content", block }],
    messageId,
    mode: "stream",
  };
  if (prev.length === 0) {
    return [freshMessage];
  }
  if (messageId) {
    const existingIndex = findLastMatchingActivityIndex(
      prev,
      (message) =>
        message.role === "assistant" &&
        message.mode === "stream" &&
        message.messageId === messageId,
    );
    if (existingIndex === prev.length - 1) {
      const next = [...prev];
      const existing = next[existingIndex];
      const settled = settleActiveThinking(existing);
      next[existingIndex] = {
        ...withContentBlock(settled, block),
        progress: undefined,
        progressKind: undefined,
        mode: "stream",
      };
      return next;
    }
  }
  const tailIndex = prev.length;
  const last = prev[tailIndex - 1];
  if (options.forceNewMessage) {
    return insertMessageAt(prev, tailIndex, freshMessage);
  }
  if (last && isEmptyStreamAssistant(last)) {
    const next = [...prev];
    next[tailIndex - 1] = freshMessage;
    return next;
  }
  if (
    !last ||
    last.role !== "assistant" ||
    last.mode !== "stream" ||
    !messageIdMatches(last, messageId)
  ) {
    return insertMessageAt(prev, tailIndex, freshMessage);
  }
  const next = [...prev];
  const settledLast = settleActiveThinking(last);
  next[tailIndex - 1] = {
    ...withContentBlock(settledLast, block),
    messageId: settledLast.messageId ?? messageId,
    progress: undefined,
    progressKind: undefined,
    mode: "stream",
  };
  return next;
}

function updateStreamAssistantMessage(
  prev: ChatMessage[],
  updater: (message: ChatMessage) => ChatMessage,
  fallback: ChatMessage,
): ChatMessage[] {
  const targetIndex = streamAssistantTargetIndex(prev);
  if (targetIndex < 0) {
    return insertMessageAt(prev, prev.length, fallback);
  }
  const next = [...prev];
  next[targetIndex] = updater(next[targetIndex]);
  return next;
}

export function appendThinkingActivityMessage(
  prev: ChatMessage[],
  block: ContentBlock,
  thinkingLabel: string,
): ChatMessage[] {
  const text = contentBlockText(block);
  if (!text) return prev;
  return updateStreamAssistantMessage(
    prev,
    (message) => {
      const activities = [...(message.activities ?? [])];
      const last = lastActivity(activities);
      if (last?.kind === "thinking" && last.active !== false) {
        activities[activities.length - 1] = {
          ...last,
          detail: `${last.detail ?? ""}${text}`,
          active: true,
        };
      } else {
        activities.push({
          id: `thinking-${Date.now()}-${activities.length}`,
          kind: "thinking",
          label: thinkingLabel,
          detail: text,
          active: true,
        });
      }
      const parts = [...(message.parts ?? [])];
      const lastPart = parts[parts.length - 1];
      if (lastPart?.kind === "thought" && lastPart.active !== false) {
        parts[parts.length - 1] = {
          ...lastPart,
          blocks: appendContentBlock(lastPart.blocks, block),
          active: true,
        };
      } else {
        parts.push({
          id: partId("thought"),
          kind: "thought",
          blocks: [block],
          active: true,
        });
      }
      return {
        ...message,
        activities,
        parts,
        progress: text,
        progressKind: "thinking",
        mode: "stream",
      };
    },
    {
      role: "assistant",
      content: "",
      progress: text,
      progressKind: "thinking",
      parts: [{ id: partId("thought"), kind: "thought", blocks: [block], active: true }],
      activities: [
        {
          id: `thinking-${Date.now()}-0`,
          kind: "thinking",
          label: thinkingLabel,
          detail: text,
          active: true,
        },
      ],
      mode: "stream",
    },
  );
}

function mergeToolCallPart(
  existing: ChatToolCallPart | undefined,
  update: ToolCallLike,
): ChatToolCallPart {
  const record = update as ToolCallLike & Record<string, unknown>;
  const toolCallId = update.toolCallId;
  const status =
    "status" in record && record.status !== undefined
      ? (record.status as ChatToolCallPart["status"])
      : existing?.status;
  return {
    id: existing?.id ?? `tool-${toolCallId}`,
    kind: "tool_call",
    toolCallId,
    title:
      "title" in record && typeof record.title === "string" && record.title.trim()
        ? record.title
        : existing?.title ?? toolActivityLabel(update),
    toolKind:
      "kind" in record && record.kind !== undefined
        ? (record.kind as ChatToolCallPart["toolKind"])
        : existing?.toolKind,
    status,
    locations:
      "locations" in record && record.locations !== undefined
        ? (record.locations as ChatToolCallPart["locations"])
        : existing?.locations,
    content:
      "content" in record && record.content !== undefined
        ? (record.content as ChatToolCallPart["content"])
        : existing?.content,
    rawInput:
      "rawInput" in record && record.rawInput !== undefined
        ? record.rawInput
        : existing?.rawInput,
    rawOutput:
      "rawOutput" in record && record.rawOutput !== undefined
        ? record.rawOutput
        : existing?.rawOutput,
    active: status !== "completed" && status !== "failed",
  };
}

function upsertToolCallPart(
  parts: ChatMessagePart[] | undefined,
  update: ToolCallLike,
): ChatMessagePart[] {
  const next = [...(parts ?? [])];
  const existingIndex = next.findIndex(
    (part) => part.kind === "tool_call" && part.toolCallId === update.toolCallId,
  );
  if (existingIndex >= 0) {
    const existing = next[existingIndex];
    if (existing.kind === "tool_call") {
      next[existingIndex] = mergeToolCallPart(existing, update);
    }
  } else {
    next.push(mergeToolCallPart(undefined, update));
  }
  return next;
}

export function appendToolActivityMessage(
  prev: ChatMessage[],
  update: ToolCallLike,
): ChatMessage[] {
  const label = toolActivityLabel(update);
  const status = toolActivityStatus(update);
  const id = toolActivityId(update);
  const active = status !== "completed" && status !== "failed";
  return updateStreamAssistantMessage(
    prev,
    (message) => {
      const settledMessage = settleActiveThinking(message);
      const activities = [...(settledMessage.activities ?? [])];
      const existingIndex =
        id !== undefined
          ? activities.findIndex((activity) => activity.id === id)
          : findLastMatchingActivity(
              activities,
              (activity) =>
                activity.kind === "tool" &&
                activity.label === label &&
                activity.active !== false,
            );
      const activity: ChatActivity = {
        id: id ?? `tool-${Date.now()}-${activities.length}`,
        kind: "tool",
        label,
        status,
        active,
      };
      if (existingIndex >= 0) {
        activities[existingIndex] = {
          ...activities[existingIndex],
          ...activity,
          id: activities[existingIndex].id,
        };
      } else {
        activities.push(activity);
      }
      return {
        ...settledMessage,
        activities,
        parts: upsertToolCallPart(settledMessage.parts, update),
        mode: "stream",
      };
    },
    {
      role: "assistant",
      content: "",
      parts: upsertToolCallPart(undefined, update),
      activities: [
        {
          id: id ?? `tool-${Date.now()}-0`,
          kind: "tool",
          label,
          status,
          active,
        },
      ],
      mode: "stream",
    },
  );
}

export function appendPlanMessage(prev: ChatMessage[], plan: Plan): ChatMessage[] {
  return updateStreamAssistantMessage(
    prev,
    (message) => {
      const settledMessage = settleActiveThinking(message);
      const parts = [...(settledMessage.parts ?? [])];
      const lastPlanIndex = findLastMatchingActivityIndex(
        parts,
        (part) => part.kind === "plan",
      );
      if (lastPlanIndex >= 0) {
        const existing = parts[lastPlanIndex];
        if (existing.kind === "plan") {
          parts[lastPlanIndex] = { ...existing, plan };
        }
      } else {
        parts.push({ id: partId("plan"), kind: "plan", plan });
      }
      return { ...settledMessage, parts, mode: "stream" };
    },
    {
      role: "assistant",
      content: "",
      parts: [{ id: partId("plan"), kind: "plan", plan }],
      mode: "stream",
    },
  );
}

function findLastMatchingActivityIndex<T>(
  items: T[],
  predicate: (item: T) => boolean,
) {
  for (let index = items.length - 1; index >= 0; index -= 1) {
    if (predicate(items[index])) return index;
  }
  return -1;
}

export function setStreamProgressMessage(
  prev: ChatMessage[],
  progress: string,
  progressKind: NonNullable<ChatMessage["progressKind"]> = "tool",
): ChatMessage[] {
  const targetIndex = streamAssistantTargetIndex(prev);
  if (targetIndex < 0) {
    return insertMessageAt(prev, prev.length, {
      role: "assistant",
      content: "",
      progress,
      progressKind,
      mode: "stream",
    });
  }
  const next = [...prev];
  next[targetIndex] = { ...next[targetIndex], progress, progressKind, mode: "stream" };
  return next;
}

export function clearStreamProgressMessage(prev: ChatMessage[]): ChatMessage[] {
  const targetIndex = streamAssistantTargetIndex(prev);
  if (targetIndex < 0) {
    return prev;
  }
  const next = [...prev];
  const settledLast = settleActiveThinking(next[targetIndex]);
  next[targetIndex] = {
    ...settledLast,
    progress: undefined,
    progressKind: undefined,
    mode: "stream",
  };
  return next;
}

export function settleStreamActivitiesMessage(prev: ChatMessage[]): ChatMessage[] {
  let changed = false;
  const next = prev.map((message) => {
    if (message.role !== "assistant" || message.mode !== "stream") return message;
    const settled = settleActiveStreamState(message);
    if (settled !== message) changed = true;
    return { ...settled, mode: "stream" as const };
  });
  return changed ? next : prev;
}

export function appendErrorToStreamMessage(
  prev: ChatMessage[],
  errorMessage: string,
): ChatMessage[] {
  const fallback: ChatMessage = {
    role: "assistant",
    content: errorMessage,
    parts: [
      {
        id: partId("content"),
        kind: "content",
        block: textContentBlock(errorMessage),
      },
    ],
    mode: "stream",
  };
  const targetIndex = streamAssistantTargetIndex(prev);
  if (targetIndex < 0) {
    return insertMessageAt(prev, prev.length, fallback);
  }
  const next = [...prev];
  const settledLast = settleActiveThinking(next[targetIndex]);
  next[targetIndex] = {
    ...withTextPart(settledLast, `${settledLast.content ? "\n\n" : ""}${errorMessage}`),
    progress: undefined,
    progressKind: undefined,
    mode: "stream",
  };
  return next;
}
