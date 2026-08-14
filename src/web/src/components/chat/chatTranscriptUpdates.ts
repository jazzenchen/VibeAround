import type { SessionNotification } from "@agentclientprotocol/sdk";

import {
  appendPlanMessage,
  appendStreamAssistantMessage,
  appendThinkingActivityMessage,
  appendToolActivityMessage,
  appendUserMessageChunk,
  clearStreamProgressMessage,
  setStreamProgressMessage,
} from "./chatMessageUpdates";
import { toolActivityLabel, toolActivityStatus } from "./chatFrameUtils";
import type { ChatMessage } from "./chatTypes";

type TranscriptUpdate = SessionNotification["update"];

export type ChatTranscriptUpdateOptions = {
  acknowledgeOptimisticUser?: boolean;
  userMessage?: {
    forceNewMessage?: boolean;
    dedupeExistingText?: boolean;
  };
  assistantMessage?: { forceNewMessage?: boolean };
  thinkingLabel?: string;
  toolProgressLabel?: (tool: string) => string;
};

export function chatIdentityChanged(
  currentIdentity: string | null,
  nextIdentity: string,
) {
  return currentIdentity !== null && currentIdentity !== nextIdentity;
}

export function applyChatTranscriptUpdate(
  messages: ChatMessage[],
  update: TranscriptUpdate,
  options: ChatTranscriptUpdateOptions = {},
): ChatMessage[] {
  switch (update.sessionUpdate) {
    case "user_message_chunk": {
      if (options.acknowledgeOptimisticUser && update.messageId) {
        const optimisticIndex = messages.findIndex(
          (message) =>
            message.role === "user" &&
            message.messageId === update.messageId &&
            message.optimistic,
        );
        if (optimisticIndex >= 0) {
          const next = [...messages];
          next[optimisticIndex] = {
            ...next[optimisticIndex],
            optimistic: false,
          };
          return next;
        }
      }
      return appendUserMessageChunk(
        messages,
        update.content,
        update.messageId,
        options.userMessage,
      );
    }
    case "agent_message_chunk":
      return appendStreamAssistantMessage(
        messages,
        update.content,
        update.messageId,
        options.assistantMessage,
      );
    case "agent_thought_chunk":
      return appendThinkingActivityMessage(
        messages,
        update.content,
        options.thinkingLabel ?? "Thinking",
      );
    case "tool_call":
    case "tool_call_update": {
      const next = appendToolActivityMessage(messages, update);
      const status = toolActivityStatus(update);
      return status === "completed" || status === "failed"
        ? clearStreamProgressMessage(next)
        : setStreamProgressMessage(
            next,
            (options.toolProgressLabel ?? defaultToolProgressLabel)(
              toolActivityLabel(update),
            ),
            "tool",
          );
    }
    case "plan":
      return appendPlanMessage(messages, update);
    default:
      return messages;
  }
}

function defaultToolProgressLabel(tool: string) {
  return `Using tool: ${tool}…`;
}
