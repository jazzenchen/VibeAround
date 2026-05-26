"use client";

import { useCallback, useEffect, useRef } from "react";
import type {
  ChatRuntimeActions,
  ChatRuntimeSnapshot,
  ChatRuntimeSpec,
} from "./chatRuntimeTypes";
import { useWebChatConnection } from "./useWebChatConnection";

interface ChatRuntimeHostProps {
  runtimeKey: string;
  initialResume?: ChatRuntimeSpec["initialResume"];
  onSnapshot: (runtimeKey: string, snapshot: ChatRuntimeSnapshot) => void;
  onActions: (runtimeKey: string, actions: ChatRuntimeActions | null) => void;
  onAgentSelected: (
    runtimeKey: string,
    agentId: string,
    source: "config" | "system",
  ) => void;
}

export function ChatRuntimeHost({
  runtimeKey,
  initialResume,
  onSnapshot,
  onActions,
  onAgentSelected,
}: ChatRuntimeHostProps) {
  const initialResumeStartedRef = useRef(false);
  const handleAgentSelected = useCallback(
    (agentId: string, source: "config" | "system") =>
      onAgentSelected(runtimeKey, agentId, source),
    [onAgentSelected, runtimeKey],
  );
  const connection = useWebChatConnection({ onAgentSelected: handleAgentSelected });

  useEffect(() => {
    onSnapshot(runtimeKey, {
      messages: connection.messages,
      connected: connection.connected,
      streaming: connection.streaming,
      meta: connection.meta,
      agents: connection.agents,
      pendingPermissions: connection.pendingPermissions,
      sessionMode: connection.sessionMode,
      resumeReplay: connection.resumeReplay,
      multiAgentTurns: connection.multiAgentTurns,
      subagents: connection.subagents,
      subagentMessages: connection.subagentMessages,
      lastPromptDoneAt: connection.lastPromptDoneAt,
    });
  }, [
    connection.agents,
    connection.connected,
    connection.lastPromptDoneAt,
    connection.messages,
    connection.meta,
    connection.multiAgentTurns,
    connection.pendingPermissions,
    connection.resumeReplay,
    connection.sessionMode,
    connection.streaming,
    connection.subagentMessages,
    connection.subagents,
    onSnapshot,
    runtimeKey,
  ]);

  useEffect(() => {
    onActions(runtimeKey, {
      sendMessage: connection.sendMessage,
      resumeSession: connection.resumeSession,
      clearConversationView: connection.clearConversationView,
      setSessionMode: connection.setSessionMode,
      setSessionConfigOption: connection.setSessionConfigOption,
      stopStreaming: connection.stopStreaming,
      sendPermissionResponse: connection.sendPermissionResponse,
      cancelPermissionRequest: connection.cancelPermissionRequest,
    });
    return () => onActions(runtimeKey, null);
  }, [
    connection.cancelPermissionRequest,
    connection.clearConversationView,
    connection.resumeSession,
    connection.sendMessage,
    connection.sendPermissionResponse,
    connection.setSessionConfigOption,
    connection.setSessionMode,
    connection.stopStreaming,
    onActions,
    runtimeKey,
  ]);

  useEffect(() => {
    if (!initialResume || initialResumeStartedRef.current || !connection.connected) {
      return;
    }
    initialResumeStartedRef.current = true;
    connection.resumeSession(initialResume);
  }, [connection.connected, connection.resumeSession, initialResume]);

  return null;
}
