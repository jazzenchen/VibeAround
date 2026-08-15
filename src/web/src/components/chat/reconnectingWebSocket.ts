import type { MutableRefObject } from "react";

const RECONNECT_DELAYS_MS = [1000, 2000, 5000, 10000];

type ReconnectingWebSocketOptions = {
  socketRef: MutableRefObject<WebSocket | null>;
  url: () => string;
  onConnecting?: () => void;
  onOpen?: () => void;
  onMessage: (event: MessageEvent) => void;
  onError?: () => void;
  onClose?: () => void;
  onCreateError?: (error: unknown) => void;
};

export function startReconnectingWebSocket({
  socketRef,
  url,
  onConnecting,
  onOpen,
  onMessage,
  onError,
  onClose,
  onCreateError,
}: ReconnectingWebSocketOptions) {
  let disposed = false;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  let reconnectAttempt = 0;

  const closeCurrentSocket = () => {
    const socket = socketRef.current;
    socketRef.current = null;
    if (!socket) return;
    socket.onopen = null;
    socket.onmessage = null;
    socket.onerror = null;
    socket.onclose = null;
    socket.close();
  };

  const scheduleReconnect = () => {
    if (disposed || reconnectTimer) return;
    const delay =
      RECONNECT_DELAYS_MS[
        Math.min(reconnectAttempt, RECONNECT_DELAYS_MS.length - 1)
      ];
    reconnectAttempt += 1;
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      connect();
    }, delay);
  };

  function connect() {
    if (disposed) return;
    closeCurrentSocket();
    onConnecting?.();

    let socket: WebSocket;
    try {
      socket = new WebSocket(url());
    } catch (error) {
      onCreateError?.(error);
      scheduleReconnect();
      return;
    }
    socketRef.current = socket;
    socket.onopen = () => {
      if (disposed || socketRef.current !== socket) return;
      reconnectAttempt = 0;
      onOpen?.();
    };
    socket.onmessage = (event) => {
      if (disposed || socketRef.current !== socket) return;
      onMessage(event);
    };
    socket.onerror = () => {
      if (disposed || socketRef.current !== socket) return;
      onError?.();
    };
    socket.onclose = () => {
      if (disposed || socketRef.current !== socket) return;
      socketRef.current = null;
      onClose?.();
      scheduleReconnect();
    };
  }

  connect();

  return () => {
    disposed = true;
    if (reconnectTimer) clearTimeout(reconnectTimer);
    closeCurrentSocket();
  };
}
