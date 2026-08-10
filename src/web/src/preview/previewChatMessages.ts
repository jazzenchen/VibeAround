import type { SessionNotification } from "@agentclientprotocol/sdk";

import type { ChatMessage } from "@/components/chat/chatTypes";
import {
  appendPlanMessage,
  appendStreamAssistantMessage,
  appendThinkingActivityMessage,
  appendToolActivityMessage,
  appendUserMessageChunk,
  clearStreamProgressMessage,
  setStreamProgressMessage,
} from "@/components/chat/chatMessageUpdates";
import { toolActivityLabel, toolActivityStatus } from "@/components/chat/chatFrameUtils";

export function previewSessionChanged(
  currentSessionId: string | null,
  nextSessionId: string,
) {
  return currentSessionId !== null && currentSessionId !== nextSessionId;
}

export function applyPreviewSessionNotification(
  messages: ChatMessage[],
  notification: SessionNotification,
) {
  const update = notification.update;
  switch (update.sessionUpdate) {
    case "user_message_chunk": {
      const optimisticIndex = update.messageId
        ? messages.findIndex(
            (message) =>
              message.role === "user" &&
              message.messageId === update.messageId &&
              message.optimistic,
          )
        : -1;
      if (optimisticIndex >= 0) {
        const next = [...messages];
        next[optimisticIndex] = { ...next[optimisticIndex], optimistic: false };
        return next;
      }
      return appendUserMessageChunk(messages, update.content, update.messageId);
    }
    case "agent_message_chunk":
      return appendStreamAssistantMessage(messages, update.content, update.messageId);
    case "agent_thought_chunk":
      return appendThinkingActivityMessage(messages, update.content, "Thinking");
    case "tool_call":
    case "tool_call_update": {
      const next = appendToolActivityMessage(messages, update);
      const status = toolActivityStatus(update);
      return status === "completed" || status === "failed"
        ? clearStreamProgressMessage(next)
        : setStreamProgressMessage(
            next,
            `Using tool: ${toolActivityLabel(update)}…`,
            "tool",
          );
    }
    case "plan":
      return appendPlanMessage(messages, update);
    default:
      return messages;
  }
}
