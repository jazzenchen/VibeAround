"use client";

import { Bot, Loader2 } from "lucide-react";
import { useI18n } from "@va/i18n";
import {
  Conversation,
  ConversationContent,
  ConversationEmptyState,
  ConversationScrollButton,
} from "./Conversation";
import { Message, MessageContent } from "./Message";
import { MessageRenderErrorBoundary } from "./MessageRenderErrorBoundary";
import { ChatMessageParts } from "./ChatMessageParts";
import { chatPartVisibleForDisplay } from "./ChatTurnDisplay";
import type { ChatDisplaySettings, ChatMessage } from "./chatTypes";

interface ChatMessageListProps {
  messages: ChatMessage[];
  streaming: boolean;
  agentLabel: string;
  replayLoading?: boolean;
  replayTitle?: string;
  displaySettings: ChatDisplaySettings;
  workspacePath?: string;
}

function activityVisible(
  activity: NonNullable<ChatMessage["activities"]>[number],
  settings: ChatDisplaySettings,
) {
  if (activity.kind === "thinking") return settings.showThinking;
  if (activity.kind === "tool") return settings.showTools;
  return true;
}

function messageVisible(message: ChatMessage, settings: ChatDisplaySettings) {
  if (message.role === "user") return true;
  if (message.parts?.some((part) => chatPartVisibleForDisplay(part, settings))) {
    return true;
  }
  if (!message.parts?.length && message.content) return true;
  if (message.activities?.some((activity) => activityVisible(activity, settings))) {
    return true;
  }
  return progressVisible(message, settings);
}

function progressVisible(message: ChatMessage, settings: ChatDisplaySettings) {
  if (!message.progress) return false;
  if (message.progressKind === "thinking") return settings.showThinking;
  return settings.showTools;
}

function messageHasVisibleActiveWork(
  message: ChatMessage | undefined,
  settings: ChatDisplaySettings,
) {
  if (!message || message.role !== "assistant") return false;
  const activePartVisible = message.parts?.some((part) => {
    if (part.kind === "thought") return part.active === true && settings.showThinking;
    if (part.kind === "tool_call") return part.active === true && settings.showTools;
    return (
      part.kind === "plan" &&
      part.plan.entries.some((entry) => entry.status === "in_progress")
    );
  });
  if (activePartVisible) return true;
  const activeActivityVisible = message.activities?.some(
    (activity) => activity.active === true && activityVisible(activity, settings),
  );
  return Boolean(activeActivityVisible || progressVisible(message, settings));
}

export function shouldShowWorkingIndicator(
  messages: ChatMessage[],
  streaming: boolean,
  settings: ChatDisplaySettings,
) {
  return streaming && !messageHasVisibleActiveWork(messages[messages.length - 1], settings);
}

function messageRenderResetKey(message: ChatMessage, index: number) {
  const partKey =
    message.parts
      ?.map((part) => {
        if (part.kind === "tool_call") {
          return `${part.id}:${part.status ?? "unknown"}:${part.content?.length ?? 0}`;
        }
        return part.id;
      })
      .join("|") ?? "";
  return [
    index,
    message.messageId ?? "",
    message.role,
    message.mode ?? "",
    message.content.length,
    message.progress ?? "",
    message.activities?.length ?? 0,
    partKey,
  ].join(":");
}

export function ChatMessageList({
  messages,
  streaming,
  agentLabel,
  replayLoading = false,
  replayTitle,
  displaySettings,
  workspacePath,
}: ChatMessageListProps) {
  const { t } = useI18n();
  const showWorkingIndicator = shouldShowWorkingIndicator(
    messages,
    streaming,
    displaySettings,
  );

  return (
    <Conversation
      className="flex-1"
      initial={streaming ? "smooth" : "instant"}
      resize={streaming ? "smooth" : "instant"}
    >
      <ConversationContent className="px-4 py-5">
        <div className="mx-auto flex w-full max-w-4xl flex-col gap-5">
          {replayLoading && (
            <div className="flex items-center gap-2 rounded-md border border-border/70 bg-muted/25 px-3 py-2 text-xs text-muted-foreground">
              <Loader2 className="h-3.5 w-3.5 animate-spin text-primary" />
              <span className="min-w-0 truncate">
                {replayTitle
                  ? t("Loading {{title}}…", { title: replayTitle })
                  : t("Loading chat history…")}
              </span>
            </div>
          )}
          {messages.length === 0 && !replayLoading && !streaming ? (
            <ConversationEmptyState
              title={t("Chat with {{agent}}", { agent: agentLabel })}
              description={t("Send a message to start.")}
              className="min-h-[50vh]"
              icon={
                <div className="flex h-14 w-14 items-center justify-center rounded-full bg-muted">
                  <Bot className="h-7 w-7 text-muted-foreground" />
                </div>
              }
            />
          ) : (
            messages.map((msg, i) => {
              const isLastStreamingMessage = streaming && i === messages.length - 1;
              if (!messageVisible(msg, displaySettings)) {
                return null;
              }
              return (
                <Message key={i} from={msg.role}>
                  <MessageContent
                    className={
                      msg.role === "user"
                        ? "max-w-[85%] rounded-lg bg-muted px-4 py-3 text-foreground sm:max-w-[34rem]"
                        : msg.mode === "standalone"
                          ? "w-full px-0 py-1 text-muted-foreground"
                          : "w-full px-0 py-1 text-foreground"
                    }
                  >
                    <MessageRenderErrorBoundary
                      resetKey={messageRenderResetKey(msg, i)}
                    >
                      <ChatMessageParts
                        message={msg}
                        isStreaming={isLastStreamingMessage}
                        displaySettings={displaySettings}
                        workspacePath={workspacePath}
                      />
                    </MessageRenderErrorBoundary>
                  </MessageContent>
                </Message>
              );
            })
          )}
          {showWorkingIndicator && (
            <Message from="assistant">
              <MessageContent className="w-full px-0 py-1 text-foreground">
                <span className="animate-pulse font-mono text-xs text-primary/80">
                  {t("AI is working…")}
                </span>
              </MessageContent>
            </Message>
          )}
        </div>
      </ConversationContent>
      <ConversationScrollButton />
    </Conversation>
  );
}
