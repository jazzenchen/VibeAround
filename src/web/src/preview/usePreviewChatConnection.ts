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
import type { ChatMessage, PendingPermission } from "@/components/chat/chatTypes";
import {
  applyPreviewSessionNotification,
  previewSessionChanged,
} from "./previewChatMessages";

const RECONNECT_DELAYS_MS = [1000, 2000, 5000, 10000];

function previewSocketUrl(slug: string) {
  const url = new URL(
    `/va/preview/u/${encodeURIComponent(slug)}/chat`,
    window.location.href,
  );
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  return url.href;
}

export function usePreviewChatConnection(slug: string, chatAvailable: boolean) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [connected, setConnected] = useState(false);
  const [streaming, setStreaming] = useState(false);
  const [agentLabel, setAgentLabel] = useState("AI");
  const [pendingPermissions, setPendingPermissions] = useState<PendingPermission[]>([]);
  const [lastTurnCompletedAt, setLastTurnCompletedAt] = useState<number>();
  const socketRef = useRef<WebSocket | null>(null);
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const reconnectAttemptRef = useRef(0);
  const turnActiveRef = useRef(false);
  const sessionIdRef = useRef<string | null>(null);

  useEffect(() => {
    let disposed = false;

    const clearReconnectTimer = () => {
      if (!reconnectTimerRef.current) return;
      clearTimeout(reconnectTimerRef.current);
      reconnectTimerRef.current = null;
    };

    const scheduleReconnect = () => {
      if (disposed || !chatAvailable || reconnectTimerRef.current) return;
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

    const resetConnectionView = () => {
      setMessages([]);
      setPendingPermissions([]);
      sessionIdRef.current = null;
      turnActiveRef.current = false;
      setStreaming(false);
    };

    const handleMessage = (event: MessageEvent) => {
      if (typeof event.data !== "string") return;
      let frame;
      try {
        frame = ChatEventSchema.parse(JSON.parse(event.data));
      } catch (error) {
        console.warn("[Preview] bad chat frame, dropping:", error);
        return;
      }

      switch (frame.kind) {
        case "agent_ready":
          setAgentLabel(frame.agent);
          break;
        case "session_ready":
          if (previewSessionChanged(sessionIdRef.current, frame.session_id)) {
            setMessages([]);
            setPendingPermissions([]);
            setLastTurnCompletedAt(undefined);
            turnActiveRef.current = false;
            setStreaming(false);
          }
          sessionIdRef.current = frame.session_id;
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
          const wasActive = turnActiveRef.current;
          turnActiveRef.current = frame.active;
          setStreaming(frame.active);
          if (wasActive && !frame.active) {
            setMessages((current) => settleStreamActivitiesMessage(current));
            setPendingPermissions([]);
            setLastTurnCompletedAt(Date.now());
          }
          break;
        }
        case "acp_notification":
          setMessages((current) =>
            applyPreviewSessionNotification(
              current,
              frame.payload as SessionNotification,
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
      if (disposed || !chatAvailable) return;
      closeSocket();
      setConnected(false);
      resetConnectionView();

      let socket: WebSocket;
      try {
        socket = new WebSocket(previewSocketUrl(slug));
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

    setLastTurnCompletedAt(undefined);
    reconnectAttemptRef.current = 0;
    if (chatAvailable) connect();
    else {
      closeSocket();
      setConnected(false);
      resetConnectionView();
    }

    return () => {
      disposed = true;
      clearReconnectTimer();
      closeSocket();
    };
  }, [chatAvailable, slug]);

  const sendMessage = useCallback(
    (text: string, displayText = text) => {
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
        socket.send(JSON.stringify({ type: "message", messageId, text: message }));
      } catch (error) {
        console.warn("[Preview] failed to send chat message:", error);
        return false;
      }
      const visibleText = displayText.trim() || message;
      setMessages((current) => [
        ...current,
        {
          role: "user",
          content: visibleText,
          messageId,
          optimistic: true,
          parts: [
            {
              id: `preview-user-${messageId}`,
              kind: "content",
              block: { type: "text", text: visibleText },
            },
          ],
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
    lastTurnCompletedAt,
    sendMessage,
    stopStreaming,
    sendPermissionResponse,
    cancelPermissionRequest,
  };
}
