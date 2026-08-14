import { useCallback, useEffect, useRef, useState } from "react";
import type { SessionNotification } from "@agentclientprotocol/sdk";
import { ChatEventSchema, formatErrorMessage } from "@va/client";

import { createMessageId } from "@/components/chat/chatFrameUtils";
import {
  appendErrorToStreamMessage,
  appendStandaloneAssistantMessage,
  setStreamProgressMessage,
  settleStreamActivitiesMessage,
} from "@/components/chat/chatMessageUpdates";
import type {
  ChatAttachment,
  ChatMessage,
  PendingPermission,
} from "@/components/chat/chatTypes";
import { chatUserContentBlocks } from "@/components/chat/chatUserContent";
import {
  applyChatTranscriptUpdate,
  chatIdentityChanged,
} from "@/components/chat/chatTranscriptUpdates";

const RECONNECT_DELAYS_MS = [1000, 2000, 5000, 10000];
const PREVIEW_THREAD_STORAGE_PREFIX = "vibearound.preview.review-thread.";

function readPreviewThreadId(slug: string): string | null {
  try {
    const threadId = window.localStorage.getItem(
      `${PREVIEW_THREAD_STORAGE_PREFIX}${slug}`,
    );
    return threadId?.trim() || null;
  } catch {
    return null;
  }
}

function writePreviewThreadId(slug: string, threadId: string) {
  try {
    window.localStorage.setItem(
      `${PREVIEW_THREAD_STORAGE_PREFIX}${slug}`,
      threadId,
    );
  } catch {
    // Storage can be unavailable in private or restricted browser contexts.
  }
}

export function previewSocketUrl(
  slug: string,
  threadId: string | null,
  pageHref = window.location.href,
) {
  const url = new URL(
    `/va/preview/u/${encodeURIComponent(slug)}/chat`,
    pageHref,
  );
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  if (threadId) url.searchParams.set("thread_id", threadId);
  return url.href;
}

export function previewConversationThreadId(value: unknown): string | null {
  if (
    typeof value !== "object" ||
    value === null ||
    !("kind" in value) ||
    value.kind !== "preview_conversation" ||
    !("thread_id" in value) ||
    typeof value.thread_id !== "string"
  ) {
    return null;
  }
  return value.thread_id.trim() || null;
}

export function previewConversationIdentityChanged(
  currentIdentity: string | null,
  nextIdentity: string,
) {
  return chatIdentityChanged(currentIdentity, nextIdentity);
}

export function usePreviewChatConnection(slug: string) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [connected, setConnected] = useState(false);
  const [streaming, setStreaming] = useState(false);
  const [agentLabel, setAgentLabel] = useState("AI");
  const [pendingPermissions, setPendingPermissions] = useState<PendingPermission[]>([]);
  const socketRef = useRef<WebSocket | null>(null);
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const reconnectAttemptRef = useRef(0);
  const turnActiveRef = useRef(false);
  const activeSlugRef = useRef<string | null>(null);
  const conversationThreadIdRef = useRef<string | null>(null);

  useEffect(() => {
    let disposed = false;

    const clearReconnectTimer = () => {
      if (!reconnectTimerRef.current) return;
      clearTimeout(reconnectTimerRef.current);
      reconnectTimerRef.current = null;
    };

    const scheduleReconnect = () => {
      if (disposed || reconnectTimerRef.current) return;
      const delay =
        RECONNECT_DELAYS_MS[
          Math.min(reconnectAttemptRef.current, RECONNECT_DELAYS_MS.length - 1)
        ];
      reconnectAttemptRef.current += 1;
      reconnectTimerRef.current = setTimeout(() => {
        reconnectTimerRef.current = null;
        connect();
      }, delay);
    };

    const closeSocket = () => {
      const socket = socketRef.current;
      socketRef.current = null;
      if (!socket) return;
      socket.onopen = null;
      socket.onmessage = null;
      socket.onerror = null;
      socket.onclose = null;
      socket.close();
    };

    const resetTransportView = () => {
      setPendingPermissions([]);
      turnActiveRef.current = false;
      setStreaming(false);
    };

    const resetConversationView = () => {
      setMessages([]);
      resetTransportView();
    };

    const handleMessage = (event: MessageEvent) => {
      if (typeof event.data !== "string") return;
      let payload: unknown;
      try {
        payload = JSON.parse(event.data);
      } catch (error) {
        console.warn("[Preview] bad chat frame, dropping:", error);
        return;
      }
      const conversationThreadId = previewConversationThreadId(payload);
      if (conversationThreadId) {
        const previousThreadId = conversationThreadIdRef.current;
        conversationThreadIdRef.current = conversationThreadId;
        writePreviewThreadId(slug, conversationThreadId);
        if (
          previewConversationIdentityChanged(
            previousThreadId,
            conversationThreadId,
          )
        ) {
          resetConversationView();
        }
        return;
      }
      let frame;
      try {
        frame = ChatEventSchema.parse(payload);
      } catch (error) {
        console.warn("[Preview] bad chat frame, dropping:", error);
        return;
      }

      switch (frame.kind) {
        case "agent_ready":
          setAgentLabel(frame.agent);
          break;
        case "system_text":
          setMessages((current) =>
            appendStandaloneAssistantMessage(current, frame.text),
          );
          break;
        case "error":
          turnActiveRef.current = false;
          setStreaming(false);
          setMessages((current) =>
            appendErrorToStreamMessage(
              current,
              `Error: ${formatErrorMessage(frame.error)}`,
            ),
          );
          break;
        case "turn_status": {
          turnActiveRef.current = frame.active;
          setStreaming(frame.active);
          if (!frame.active) {
            setMessages((current) => settleStreamActivitiesMessage(current));
            setPendingPermissions([]);
          }
          break;
        }
        case "acp_notification":
          setMessages((current) =>
            applyChatTranscriptUpdate(
              current,
              (frame.payload as SessionNotification).update,
              { acknowledgeOptimisticUser: true },
            ),
          );
          break;
        case "permission_request":
          setPendingPermissions((current) => [
            ...current.filter(
              (permission) => permission.requestId !== frame.request_id,
            ),
            { requestId: frame.request_id, request: frame.request },
          ]);
          break;
        default:
          break;
      }
    };

    function connect() {
      if (disposed) return;
      closeSocket();
      setConnected(false);
      resetTransportView();

      let socket: WebSocket;
      try {
        socket = new WebSocket(
          previewSocketUrl(slug, conversationThreadIdRef.current),
        );
      } catch (error) {
        console.warn("[Preview] failed to create chat websocket:", error);
        scheduleReconnect();
        return;
      }
      socketRef.current = socket;
      socket.onopen = () => {
        if (disposed || socketRef.current !== socket) return;
        reconnectAttemptRef.current = 0;
        setConnected(true);
      };
      socket.onmessage = handleMessage;
      socket.onerror = () => {
        if (socketRef.current === socket) setConnected(false);
      };
      socket.onclose = () => {
        if (disposed || socketRef.current !== socket) return;
        socketRef.current = null;
        setConnected(false);
        turnActiveRef.current = false;
        setStreaming(false);
        setMessages((current) => settleStreamActivitiesMessage(current));
        scheduleReconnect();
      };
    }

    if (previewConversationIdentityChanged(activeSlugRef.current, slug)) {
      resetConversationView();
    }
    activeSlugRef.current = slug;
    reconnectAttemptRef.current = 0;
    conversationThreadIdRef.current = readPreviewThreadId(slug);
    connect();

    return () => {
      disposed = true;
      clearReconnectTimer();
      closeSocket();
    };
  }, [slug]);

  const sendMessage = useCallback(
    (text: string, displayText = text, attachments: ChatAttachment[] = []) => {
      const message = text.trim();
      const socket = socketRef.current;
      if (
        !message ||
        !socket ||
        socket.readyState !== WebSocket.OPEN ||
        turnActiveRef.current
      ) {
        return false;
      }
      const messageId = createMessageId();
      try {
        socket.send(
          JSON.stringify({
            type: "message",
            messageId,
            text: message,
            attachments: attachments.map((attachment) => ({
              id: attachment.id,
              name: attachment.name,
              mimeType: attachment.mimeType,
              size: attachment.size,
              uri: attachment.uri,
            })),
          }),
        );
      } catch (error) {
        console.warn("[Preview] failed to send chat message:", error);
        return false;
      }
      const visibleText = displayText.trim() || message;
      const contentParts = chatUserContentBlocks(visibleText, attachments).map(
        (block, index) => ({
          id: `preview-user-${messageId}-${index}`,
          kind: "content" as const,
          block,
        }),
      );
      setMessages((current) => [
        ...current,
        {
          role: "user",
          content: visibleText,
          messageId,
          optimistic: true,
          parts: contentParts,
        },
      ]);
      turnActiveRef.current = true;
      setStreaming(true);
      return true;
    },
    [],
  );

  const stopStreaming = useCallback(() => {
    const socket = socketRef.current;
    if (!turnActiveRef.current || !socket || socket.readyState !== WebSocket.OPEN) {
      return false;
    }
    try {
      socket.send(JSON.stringify({ type: "stop" }));
      setMessages((current) =>
        setStreamProgressMessage(current, "Stopping…", "tool"),
      );
      return true;
    } catch (error) {
      console.warn("[Preview] failed to stop chat message:", error);
      return false;
    }
  }, []);

  const sendPermissionResponse = useCallback(
    (requestId: string, optionId: string) => {
      const socket = socketRef.current;
      if (!socket || socket.readyState !== WebSocket.OPEN) return;
      socket.send(
        JSON.stringify({ type: "permission_response", requestId, optionId }),
      );
      setPendingPermissions((current) =>
        current.filter((permission) => permission.requestId !== requestId),
      );
    },
    [],
  );

  const cancelPermissionRequest = useCallback((requestId: string) => {
    const socket = socketRef.current;
    if (!socket || socket.readyState !== WebSocket.OPEN) return;
    socket.send(
      JSON.stringify({
        type: "permission_response",
        requestId,
        outcome: "cancelled",
      }),
    );
    setPendingPermissions((current) =>
      current.filter((permission) => permission.requestId !== requestId),
    );
  }, []);

  return {
    messages,
    connected,
    streaming,
    agentLabel,
    pendingPermissions,
    sendMessage,
    stopStreaming,
    sendPermissionResponse,
    cancelPermissionRequest,
  };
}
