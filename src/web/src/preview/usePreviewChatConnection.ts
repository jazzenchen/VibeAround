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
import { applyChatTranscriptUpdate } from "@/components/chat/chatTranscriptUpdates";
import { startReconnectingWebSocket } from "@/components/chat/reconnectingWebSocket";

export function previewSocketUrl(
  slug: string,
  pageHref = window.location.href,
) {
  const url = new URL(
    `/va/preview/u/${encodeURIComponent(slug)}/chat`,
    pageHref,
  );
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  return url.href;
}

export function usePreviewChatConnection(
  slug: string,
  onPreviewRefresh: () => void | Promise<void>,
) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [connected, setConnected] = useState(false);
  const [streaming, setStreaming] = useState(false);
  const [agentLabel, setAgentLabel] = useState("AI");
  const [pendingPermissions, setPendingPermissions] = useState<PendingPermission[]>([]);
  const socketRef = useRef<WebSocket | null>(null);
  const turnActiveRef = useRef(false);

  useEffect(() => {
    const resetTransportView = () => {
      setPendingPermissions([]);
      turnActiveRef.current = false;
      setStreaming(false);
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
        case "preview_refresh":
          void onPreviewRefresh();
          break;
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

    setMessages([]);
    resetTransportView();

    return startReconnectingWebSocket({
      socketRef,
      url: () => previewSocketUrl(slug),
      onConnecting: () => {
        setConnected(false);
        resetTransportView();
      },
      onOpen: () => setConnected(true),
      onMessage: handleMessage,
      onError: () => setConnected(false),
      onClose: () => {
        setConnected(false);
        resetTransportView();
        setMessages((current) => settleStreamActivitiesMessage(current));
      },
      onCreateError: (error) =>
        console.warn("[Preview] failed to create chat websocket:", error),
    });
  }, [onPreviewRefresh, slug]);

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
