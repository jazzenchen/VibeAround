"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import type {
  ContentBlock,
  Plan,
  SessionNotification,
  ToolCall,
  ToolCallUpdate,
} from "@agentclientprotocol/sdk";
import {
  ChatEventSchema,
  type AgentInfo,
  type LaunchSessionInfo,
  type MultiAgentTurn,
  type ThreadAgent,
} from "@va/client";
import { useI18n } from "@va/i18n";
import { getWebSocketUrl } from "@/lib/ws-url";
import type {
  ChatAttachment,
  ChatMessage,
  ChatMeta,
  ChatSessionSelection,
  PendingPermission,
  SessionModeState,
} from "./chatTypes";
import {
  createMessageId,
  switchedAgentId,
  toolActivityLabel,
  toolActivityStatus,
} from "./chatFrameUtils";
import {
  appendErrorToStreamMessage,
  appendPlanMessage,
  appendStandaloneAssistantMessage,
  appendStreamAssistantMessage,
  appendThinkingActivityMessage,
  appendToolActivityMessage,
  appendUserMessageChunk,
  clearStreamProgressMessage,
  mergeChatMessageSnapshots,
  setStreamProgressMessage,
  settleStreamActivitiesMessage,
} from "./chatMessageUpdates";
import {
  readCachedChatSession,
  writeCachedChatSession,
} from "./chatSessionCache";
import {
  parseModeFromConfigOptions,
  parseSessionModeState,
} from "./chatSessionMode";
import { currentUnixSeconds } from "./chatTime";

interface UseWebChatConnectionOptions {
  onAgentSelected?: (agentId: string, source: "config" | "system") => void;
}

const CACHE_WRITE_DEBOUNCE_MS = 350;
const RESUME_REPLAY_SETTLE_MS = 700;
const USER_CONTENT_PART_ID_PREFIX = "user-content";
const COMPACTION_NOTICE_DROP_RATIO = 0.55;
const COMPACTION_NOTICE_MIN_WINDOW_RATIO = 0.25;
const COMPACTION_NOTICE_MIN_DROP = 128;

interface SendChatMessageRequest {
  text: string;
  attachments?: ChatAttachment[];
  agentId: string;
  profileId?: string;
  workspacePath?: string;
  sessionSelection: ChatSessionSelection;
  launchSession?: LaunchSessionInfo;
}

interface ResumeChatSessionRequest {
  agentId: string;
  profileId?: string;
  launchSession: LaunchSessionInfo;
}

type MessageUpdate = (prev: ChatMessage[]) => ChatMessage[];

type UsageSnapshot = {
  used: number;
  size: number;
};

type SessionUpdateLike = {
  sessionUpdate?: unknown;
  _meta?: unknown;
  used?: unknown;
  size?: unknown;
};

function asRecord(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  return value as Record<string, unknown>;
}

function sessionUpdateName(update: unknown): string | undefined {
  const record = asRecord(update);
  const name = record?.sessionUpdate;
  return typeof name === "string" ? name : undefined;
}

function hasCompactionSignal(value: unknown, depth = 0): boolean {
  if (depth > 4 || value === null || value === undefined) return false;
  if (typeof value === "string") {
    return value.toLowerCase().includes("compact");
  }
  if (typeof value !== "object") return false;
  if (Array.isArray(value)) {
    return value.some((item) => hasCompactionSignal(item, depth + 1));
  }
  return Object.entries(value).some(
    ([key, item]) =>
      key.toLowerCase().includes("compact") ||
      hasCompactionSignal(item, depth + 1),
  );
}

function isCompactionUpdate(update: unknown): boolean {
  const name = sessionUpdateName(update);
  if (name?.toLowerCase().includes("compact")) return true;
  return hasCompactionSignal(asRecord(update)?._meta);
}

function usageSnapshot(update: unknown): UsageSnapshot | null {
  const record = asRecord(update) as SessionUpdateLike | null;
  if (!record || record.sessionUpdate !== "usage_update") return null;
  if (typeof record.used !== "number" || typeof record.size !== "number") return null;
  if (!Number.isFinite(record.used) || !Number.isFinite(record.size)) return null;
  if (record.used < 0 || record.size <= 0) return null;
  return { used: record.used, size: record.size };
}

function isCompactionUsageDrop(
  previous: UsageSnapshot | undefined,
  current: UsageSnapshot,
): boolean {
  if (!previous || previous.size !== current.size || previous.used <= current.used) {
    return false;
  }
  const drop = previous.used - current.used;
  const enoughDrop = drop >= Math.max(COMPACTION_NOTICE_MIN_DROP, current.size * 0.02);
  const enoughPriorContext =
    previous.used >= current.size * COMPACTION_NOTICE_MIN_WINDOW_RATIO;
  const sharpReset = current.used / previous.used <= COMPACTION_NOTICE_DROP_RATIO;
  return enoughDrop && enoughPriorContext && sharpReset;
}

interface TranscriptCacheContext {
  sessionId?: string;
  agentId?: string;
  workspace?: string;
  updatedAt?: number;
}

export interface ResumeReplayState {
  sessionId: string;
  title?: string;
  agentId?: string;
  workspace?: string;
  updatedAt?: number;
  blocking?: boolean;
}

export function useWebChatConnection({
  onAgentSelected,
}: UseWebChatConnectionOptions = {}) {
  const { t } = useI18n();
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [connected, setConnected] = useState(false);
  const [streaming, setStreaming] = useState(false);
  const [meta, setMeta] = useState<ChatMeta>({});
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [pendingPermissions, setPendingPermissions] = useState<PendingPermission[]>([]);
  const [sessionMode, setSessionModeState] = useState<SessionModeState | null>(null);
  const [resumeReplay, setResumeReplay] = useState<ResumeReplayState | null>(null);
  const [multiAgentTurns, setMultiAgentTurns] = useState<MultiAgentTurn[]>([]);
  const [subagents, setSubagents] = useState<ThreadAgent[]>([]);
  const [subagentMessages, setSubagentMessages] = useState<
    Record<string, ChatMessage[]>
  >({});
  const [lastPromptDoneAt, setLastPromptDoneAt] = useState<number | undefined>();
  const wsRef = useRef<WebSocket | null>(null);
  const promptInFlightRef = useRef(false);
  const resumeReplayRef = useRef<ResumeReplayState | null>(null);
  const resumeRequestIdRef = useRef(0);
  const cancelReplayOnNextTurnRef = useRef(false);
  const messagesRef = useRef<ChatMessage[]>([]);
  const replayMessageBufferRef = useRef<{
    sessionId: string;
    messages: ChatMessage[];
  } | null>(null);
  const ignoredReplaySessionsRef = useRef<Set<string>>(new Set());
  const usageBySessionRef = useRef<Map<string, UsageSnapshot>>(new Map());
  const compactionNoticeKeysRef = useRef<Set<string>>(new Set());
  const resumeReplayDoneTimerRef = useRef<ReturnType<typeof setTimeout> | null>(
    null,
  );
  const replayCacheContextRef = useRef<ResumeReplayState | null>(null);
  const replayCacheWriteTimerRef = useRef<ReturnType<typeof setTimeout> | null>(
    null,
  );
  const activeTranscriptCacheRef = useRef<TranscriptCacheContext | null>(null);
  const activeTranscriptCacheWriteTimerRef = useRef<ReturnType<
    typeof setTimeout
  > | null>(null);

  const updateResumeReplay = useCallback((next: ResumeReplayState | null) => {
    resumeReplayRef.current = next;
    setResumeReplay(next);
  }, []);

  const clearResumeReplayDoneTimer = useCallback(() => {
    if (!resumeReplayDoneTimerRef.current) return;
    clearTimeout(resumeReplayDoneTimerRef.current);
    resumeReplayDoneTimerRef.current = null;
  }, []);

  const clearReplayCacheWriteTimer = useCallback(() => {
    if (!replayCacheWriteTimerRef.current) return;
    clearTimeout(replayCacheWriteTimerRef.current);
    replayCacheWriteTimerRef.current = null;
  }, []);

  const clearActiveTranscriptCacheWriteTimer = useCallback(() => {
    if (!activeTranscriptCacheWriteTimerRef.current) return;
    clearTimeout(activeTranscriptCacheWriteTimerRef.current);
    activeTranscriptCacheWriteTimerRef.current = null;
  }, []);

  const clearReplayCacheContext = useCallback(() => {
    replayCacheContextRef.current = null;
    clearReplayCacheWriteTimer();
  }, [clearReplayCacheWriteTimer]);

  const cacheTranscript = useCallback((
    context: TranscriptCacheContext,
    messagesToCache = messagesRef.current,
  ) => {
    if (
      !context.sessionId ||
      !context.agentId ||
      !context.workspace ||
      context.updatedAt === undefined
    ) {
      return;
    }
    if (messagesToCache.length === 0) return;
    void writeCachedChatSession({
      agentId: context.agentId,
      workspace: context.workspace,
      sessionId: context.sessionId,
      updatedAt: context.updatedAt,
      messages: messagesToCache,
    }).catch((error) => {
      console.warn("[ChatView] failed to cache chat session:", error);
    });
  }, []);

  const cacheResumeReplay = useCallback((
    replay: ResumeReplayState,
    messagesToCache = messagesRef.current,
  ) => {
    cacheTranscript(replay, messagesToCache);
  }, [cacheTranscript]);

  const scheduleReplayCacheWrite = useCallback(
    (replay = replayCacheContextRef.current) => {
      if (!replay?.agentId || !replay.workspace || replay.updatedAt === undefined) {
        return;
      }
      if (messagesRef.current.length === 0) return;
      clearReplayCacheWriteTimer();
      replayCacheWriteTimerRef.current = setTimeout(() => {
        cacheResumeReplay(replay);
      }, CACHE_WRITE_DEBOUNCE_MS);
    },
    [cacheResumeReplay, clearReplayCacheWriteTimer],
  );

  const scheduleActiveTranscriptCacheWrite = useCallback(
    (context = activeTranscriptCacheRef.current) => {
      if (
        !context?.sessionId ||
        !context.agentId ||
        !context.workspace ||
        context.updatedAt === undefined
      ) {
        return;
      }
      if (messagesRef.current.length === 0) return;
      clearActiveTranscriptCacheWriteTimer();
      activeTranscriptCacheWriteTimerRef.current = setTimeout(() => {
        cacheTranscript(context);
      }, CACHE_WRITE_DEBOUNCE_MS);
    },
    [cacheTranscript, clearActiveTranscriptCacheWriteTimer],
  );

  const applyMessageUpdate = useCallback((
    updater: MessageUpdate,
    replaySessionId?: string,
  ) => {
    const replayBuffer = replaySessionId ? replayMessageBufferRef.current : null;
    if (replayBuffer && replayBuffer.sessionId === replaySessionId) {
      replayBuffer.messages = updater(replayBuffer.messages);
      return;
    }
    setMessages(updater);
  }, []);

  useEffect(() => {
    messagesRef.current = messages;
    scheduleReplayCacheWrite();
    scheduleActiveTranscriptCacheWrite();
  }, [messages, scheduleActiveTranscriptCacheWrite, scheduleReplayCacheWrite]);

  const finishResumeReplay = useCallback(
    (sessionId?: string, options?: { cache?: boolean }) => {
      const current = resumeReplayRef.current;
      if (sessionId && current?.sessionId !== sessionId) return;
      if (current && options?.cache) {
        const replayBuffer = replayMessageBufferRef.current;
        if (replayBuffer?.sessionId === current.sessionId) {
          const replayedMessages = settleStreamActivitiesMessage(
            replayBuffer.messages,
          );
          replayMessageBufferRef.current = null;
          if (replayedMessages.length > 0) {
            const mergedMessages = mergeChatMessageSnapshots(
              messagesRef.current,
              replayedMessages,
            );
            messagesRef.current = mergedMessages;
            setMessages(mergedMessages);
            cacheResumeReplay(current, mergedMessages);
          }
          clearReplayCacheContext();
        } else {
          scheduleReplayCacheWrite(current);
        }
      } else if (!options?.cache) {
        replayMessageBufferRef.current = null;
        clearReplayCacheContext();
      }
      clearResumeReplayDoneTimer();
      updateResumeReplay(null);
    },
    [
      cacheResumeReplay,
      clearReplayCacheContext,
      clearResumeReplayDoneTimer,
      scheduleReplayCacheWrite,
      updateResumeReplay,
    ],
  );

  const scheduleResumeReplayDone = useCallback(
    (sessionId: string) => {
      const currentReplay = resumeReplayRef.current;
      if (currentReplay && currentReplay.sessionId !== sessionId) return;
      if (resumeReplayDoneTimerRef.current) return;
      resumeReplayDoneTimerRef.current = setTimeout(() => {
        finishResumeReplay(sessionId, { cache: true });
      }, RESUME_REPLAY_SETTLE_MS);
    },
    [finishResumeReplay],
  );

  useEffect(() => {
    const ws = new WebSocket(getWebSocketUrl("/ws/chat"));
    wsRef.current = ws;

    ws.onopen = () => setConnected(true);
    ws.onclose = () => {
      setConnected(false);
      setStreaming(false);
      setMessages((prev) => settleStreamActivitiesMessage(prev));
      promptInFlightRef.current = false;
      setPendingPermissions([]);
      finishResumeReplay();
    };
    ws.onerror = () => {
      setConnected(false);
      setStreaming(false);
      setMessages((prev) => settleStreamActivitiesMessage(prev));
      promptInFlightRef.current = false;
      finishResumeReplay();
    };

    ws.onmessage = (event) => {
      if (typeof event.data !== "string") return;

      let parsed;
      try {
        parsed = ChatEventSchema.parse(JSON.parse(event.data));
      } catch (e) {
        console.warn("[ChatView] bad chat frame, dropping:", e);
        return;
      }

      switch (parsed.kind) {
        case "config": {
          setAgents(parsed.agents);
          setMeta((prev) => ({ ...prev, channelId: parsed.channel_id }));
          onAgentSelected?.(parsed.default_agent, "config");
          break;
        }
        case "agent_ready": {
          setMeta((prev) => ({
            ...prev,
            agentName: parsed.agent,
            agentVersion: parsed.version,
          }));
          break;
        }
        case "session_ready": {
          const pendingResume = resumeReplayRef.current;
          if (!pendingResume && ignoredReplaySessionsRef.current.has(parsed.session_id)) {
            break;
          }
          if (pendingResume && pendingResume.sessionId !== parsed.session_id) {
            break;
          }
          setMeta((prev) => ({ ...prev, sessionId: parsed.session_id }));
          setSessionModeState(null);
          if (activeTranscriptCacheRef.current) {
            activeTranscriptCacheRef.current = {
              ...activeTranscriptCacheRef.current,
              sessionId: parsed.session_id,
            };
            cacheTranscript(activeTranscriptCacheRef.current);
          }
          if (pendingResume?.sessionId === parsed.session_id) {
            scheduleResumeReplayDone(parsed.session_id);
          }
          break;
        }
        case "session_mode": {
          setSessionModeState(parseSessionModeState(parsed.session_mode));
          break;
        }
        case "system_text": {
          appendStandaloneAssistant(parsed.text);
          finishResumeReplay();
          const agentId = switchedAgentId(parsed.text);
          if (agentId) {
            onAgentSelected?.(agentId, "system");
            setMeta((prev) => ({
              ...prev,
              agentName: undefined,
              agentTitle: undefined,
              agentVersion: undefined,
              sessionId: undefined,
            }));
          }
          break;
        }
        case "error": {
          appendErrorToStream(parsed.error);
          setStreaming(false);
          promptInFlightRef.current = false;
          finishResumeReplay();
          break;
        }
        case "prompt_done": {
          settleStreamActivities();
          setStreaming(false);
          promptInFlightRef.current = false;
          if (activeTranscriptCacheRef.current) {
            activeTranscriptCacheRef.current = {
              ...activeTranscriptCacheRef.current,
              updatedAt: Math.max(
                activeTranscriptCacheRef.current.updatedAt ?? 0,
                currentUnixSeconds(),
              ),
            };
            cacheTranscript(activeTranscriptCacheRef.current);
          }
          setLastPromptDoneAt(Date.now());
          break;
        }
        case "turn_status": {
          if (parsed.active && cancelReplayOnNextTurnRef.current) {
            cancelReplayOnNextTurnRef.current = false;
            finishResumeReplay(undefined, { cache: false });
          }
          setStreaming(parsed.active);
          promptInFlightRef.current = parsed.active;
          break;
        }
        case "acp_notification": {
          handleAcpNotification(parsed.payload as SessionNotification);
          break;
        }
        case "command_menu":
          break;
        case "permission_request": {
          setPendingPermissions((prev) => [
            ...prev.filter((permission) => permission.requestId !== parsed.request_id),
            { requestId: parsed.request_id, request: parsed.request },
          ]);
          break;
        }
        case "multi_agent_turn": {
          setMultiAgentTurns((prev) => mergeById(prev, [parsed.turn]));
          setSubagents((prev) => mergeById(prev, parsed.agents));
          break;
        }
        case "subagent_status": {
          setSubagents((prev) => mergeById(prev, [parsed.agent]));
          break;
        }
        case "subagent_acp_notification": {
          const notif = parsed.payload as SessionNotification;
          setSubagentMessages((prev) => ({
            ...prev,
            [parsed.agent.id]: applySubagentAcpNotification(
              prev[parsed.agent.id] ?? [],
              notif,
            ),
          }));
          break;
        }
      }
    };

    function handleAcpNotification(notif: SessionNotification) {
      if (ignoredReplaySessionsRef.current.has(notif.sessionId)) {
        return;
      }
      const pendingResume = resumeReplayRef.current;
      if (pendingResume && notif.sessionId !== pendingResume.sessionId) {
        return;
      }
      const replaying = pendingResume?.sessionId === notif.sessionId;
      const replaySessionId = replaying ? notif.sessionId : undefined;

      const update = notif.update;
      const usage = usageSnapshot(update);
      if (usage) {
        const previous = usageBySessionRef.current.get(notif.sessionId);
        usageBySessionRef.current.set(notif.sessionId, usage);
        if (isCompactionUsageDrop(previous, usage)) {
          appendCompactionNotice(
            notif.sessionId,
            `usage:${previous?.used}->${usage.used}/${usage.size}`,
            replaySessionId,
          );
        }
      }
      if (isCompactionUpdate(update)) {
        appendCompactionNotice(
          notif.sessionId,
          `update:${sessionUpdateName(update) ?? "meta"}:${usage?.used ?? ""}/${usage?.size ?? ""}`,
          replaySessionId,
        );
      }

      switch (update.sessionUpdate) {
        case "user_message_chunk": {
          appendUserMessage(update.content, update.messageId, {
            forceNewMessage: replaying && !update.messageId,
            dedupeExistingText: !replaying,
          }, replaySessionId);
          if (replaying) scheduleResumeReplayDone(notif.sessionId);
          break;
        }
        case "agent_message_chunk": {
          appendToStreamAssistant(update.content, update.messageId, replaySessionId);
          if (replaying) scheduleResumeReplayDone(notif.sessionId);
          break;
        }
        case "agent_thought_chunk": {
          appendThinkingActivity(update.content, replaySessionId);
          if (replaying) scheduleResumeReplayDone(notif.sessionId);
          break;
        }
        case "tool_call":
        case "tool_call_update": {
          const title = toolActivityLabel(update);
          const status = toolActivityStatus(update);
          appendToolActivity(update, replaySessionId);
          if (status === "completed" || status === "failed") {
            clearStreamProgress(replaySessionId);
          } else {
            setStreamProgress(
              t("Using tool: {{tool}}…", { tool: title }),
              replaySessionId,
            );
          }
          if (replaying) scheduleResumeReplayDone(notif.sessionId);
          break;
        }
        case "plan": {
          appendPlan(update, replaySessionId);
          if (replaying) scheduleResumeReplayDone(notif.sessionId);
          break;
        }
        case "config_option_update": {
          setSessionModeState(
            parseModeFromConfigOptions(
              (update as { configOptions?: unknown }).configOptions,
            ),
          );
          if (replaying) scheduleResumeReplayDone(notif.sessionId);
          break;
        }
        case "current_mode_update": {
          const modeId = (update as { modeId?: unknown }).modeId;
          if (typeof modeId === "string" && modeId.trim()) {
            setSessionModeState((prev) =>
              prev?.source === "session_mode"
                ? { ...prev, currentValue: modeId.trim() }
                : prev,
            );
          }
          if (replaying) scheduleResumeReplayDone(notif.sessionId);
          break;
        }
        // Other ACP update variants (available_commands_update, mode/config,
        // session metadata, usage, etc.) update surrounding UI rather than the
        // visible transcript. Ignored here so future SDK additions don't crash
        // the handler.
        default:
          if (replaying) scheduleResumeReplayDone(notif.sessionId);
          break;
      }
    }

    function appendCompactionNotice(
      sessionId: string,
      noticeKey: string,
      replaySessionId?: string,
    ) {
      const key = `${sessionId}:${noticeKey}`;
      if (compactionNoticeKeysRef.current.has(key)) return;
      compactionNoticeKeysRef.current.add(key);
      appendStandaloneAssistant(
        t("Context compacted. Continuing from a compressed summary."),
        replaySessionId,
      );
    }

    function appendStandaloneAssistant(text: string, replaySessionId?: string) {
      applyMessageUpdate(
        (prev) => appendStandaloneAssistantMessage(prev, text),
        replaySessionId,
      );
    }

    function appendUserMessage(
      content: ContentBlock,
      messageId?: string | null,
      options?: { forceNewMessage?: boolean; dedupeExistingText?: boolean },
      replaySessionId?: string,
    ) {
      applyMessageUpdate(
        (prev) => appendUserMessageChunk(prev, content, messageId, options),
        replaySessionId,
      );
    }

    function appendToStreamAssistant(
      content: ContentBlock,
      messageId?: string | null,
      replaySessionId?: string,
      options?: { forceNewMessage?: boolean },
    ) {
      applyMessageUpdate(
        (prev) => appendStreamAssistantMessage(prev, content, messageId, options),
        replaySessionId,
      );
    }

    function appendThinkingActivity(content: ContentBlock, replaySessionId?: string) {
      applyMessageUpdate(
        (prev) => appendThinkingActivityMessage(prev, content, t("Thinking")),
        replaySessionId,
      );
    }

    function appendToolActivity(
      update: ToolCall | ToolCallUpdate,
      replaySessionId?: string,
    ) {
      applyMessageUpdate(
        (prev) => appendToolActivityMessage(prev, update),
        replaySessionId,
      );
    }

    function appendPlan(plan: Plan, replaySessionId?: string) {
      applyMessageUpdate((prev) => appendPlanMessage(prev, plan), replaySessionId);
    }

    function setStreamProgress(progress: string, replaySessionId?: string) {
      applyMessageUpdate(
        (prev) => setStreamProgressMessage(prev, progress, "tool"),
        replaySessionId,
      );
    }

    function clearStreamProgress(replaySessionId?: string) {
      applyMessageUpdate((prev) => clearStreamProgressMessage(prev), replaySessionId);
    }

    function settleStreamActivities(replaySessionId?: string) {
      applyMessageUpdate((prev) => settleStreamActivitiesMessage(prev), replaySessionId);
    }

    function appendErrorToStream(error: string) {
      applyMessageUpdate(
        (prev) => appendErrorToStreamMessage(prev, t("Error: {{error}}", { error })),
      );
    }

    return () => {
      ws.close();
      wsRef.current = null;
      clearResumeReplayDoneTimer();
      clearReplayCacheWriteTimer();
      clearActiveTranscriptCacheWriteTimer();
    };
  }, [
    applyMessageUpdate,
    cacheTranscript,
    clearActiveTranscriptCacheWriteTimer,
    clearReplayCacheWriteTimer,
    clearResumeReplayDoneTimer,
    finishResumeReplay,
    onAgentSelected,
    scheduleResumeReplayDone,
    t,
  ]);

  const sendMessage = useCallback(
    ({
      text,
      attachments = [],
      agentId,
      profileId,
      workspacePath,
      sessionSelection,
      launchSession,
    }: SendChatMessageRequest) => {
      const trimmed = text.trim();
      const ws = wsRef.current;
      if ((!trimmed && attachments.length === 0) || !ws || ws.readyState !== WebSocket.OPEN) {
        return false;
      }
      if (promptInFlightRef.current) return false;

      promptInFlightRef.current = true;
      const messageId = createMessageId();
      const contentParts = messageContentBlocks(trimmed, attachments).map((block, index) => ({
        id: `${USER_CONTENT_PART_ID_PREFIX}-${Date.now()}-${index}`,
        kind: "content" as const,
        block,
      }));
      const optimisticMessage: ChatMessage = {
        role: "user",
        content: trimmed,
        parts: contentParts,
        messageId,
        optimistic: true,
      };
      const optimisticMessages = [...messagesRef.current, optimisticMessage];
      messagesRef.current = optimisticMessages;
      setMessages(optimisticMessages);
      setStreaming(true);
      const submittedAt = currentUnixSeconds();

      try {
        const payload: Record<string, unknown> = {
          type: "message",
          messageId,
          text: trimmed,
          agent: agentId,
        };
        if (attachments.length > 0) {
          payload.attachments = attachments.map((attachment) => ({
            id: attachment.id,
            name: attachment.name,
            mimeType: attachment.mimeType,
            size: attachment.size,
            uri: attachment.uri,
          }));
        }
        if (profileId !== undefined) {
          payload.profileId = profileId;
        }
        if (sessionSelection.kind === "new") {
          payload.sessionAction = "new";
          if (workspacePath) {
            payload.sessionWorkspace = workspacePath;
          }
        } else if (sessionSelection.kind === "resume" && launchSession) {
          payload.sessionAction = "resume";
          payload.sessionId = launchSession.session_id;
          payload.sessionWorkspace = launchSession.workspace;
        }
        ws.send(JSON.stringify(payload));
        const cacheContext: TranscriptCacheContext = {
          sessionId: launchSession?.session_id,
          agentId,
          workspace: launchSession?.workspace ?? workspacePath,
          updatedAt: Math.max(launchSession?.updated_at ?? 0, submittedAt),
        };
        activeTranscriptCacheRef.current = cacheContext;
        cacheTranscript(cacheContext);
        if (resumeReplayRef.current) {
          cancelReplayOnNextTurnRef.current = true;
        }
        clearReplayCacheContext();
        return true;
      } catch (error) {
        console.warn("[ChatView] failed to send chat message:", error);
        cancelReplayOnNextTurnRef.current = false;
        activeTranscriptCacheRef.current = null;
        clearActiveTranscriptCacheWriteTimer();
        promptInFlightRef.current = false;
        setStreaming(false);
        setMessages((prev) => prev.filter((message) => message.messageId !== messageId));
        return false;
      }
    },
    [cacheTranscript, clearActiveTranscriptCacheWriteTimer, clearReplayCacheContext],
  );

  const setSessionMode = useCallback((modeId: string) => {
    const ws = wsRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN) return false;
    try {
      ws.send(JSON.stringify({ type: "set_mode", modeId }));
      return true;
    } catch (error) {
      console.warn("[ChatView] failed to set session mode:", error);
      return false;
    }
  }, []);

  const setSessionConfigOption = useCallback((configId: string, value: string) => {
    const ws = wsRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN) return false;
    try {
      ws.send(JSON.stringify({ type: "set_config_option", configId, value }));
      return true;
    } catch (error) {
      console.warn("[ChatView] failed to set session config option:", error);
      return false;
    }
  }, []);

  const clearConversationView = useCallback((options?: {
    abortReplay?: boolean;
    preserveMessages?: boolean;
    sendStop?: boolean;
  }) => {
    const ws = wsRef.current;
    const replayContext = resumeReplayRef.current ?? replayCacheContextRef.current;
    const abortedSessionId = replayContext?.sessionId;
    if (options?.abortReplay) {
      resumeRequestIdRef.current += 1;
    }
    if (
      options?.abortReplay &&
      options.sendStop !== false &&
      replayContext &&
      ws?.readyState === WebSocket.OPEN
    ) {
      try {
        ws.send(JSON.stringify({ type: "stop" }));
      } catch (error) {
        console.warn("[ChatView] failed to abort session replay:", error);
      }
    }
    if (options?.abortReplay && abortedSessionId) {
      ignoredReplaySessionsRef.current.add(abortedSessionId);
    }
    clearResumeReplayDoneTimer();
    clearReplayCacheContext();
    activeTranscriptCacheRef.current = null;
    clearActiveTranscriptCacheWriteTimer();
    replayMessageBufferRef.current = null;
    cancelReplayOnNextTurnRef.current = false;
    promptInFlightRef.current = false;
    setStreaming(false);
    setPendingPermissions([]);
    updateResumeReplay(null);
    if (!options?.preserveMessages) {
      messagesRef.current = [];
      setMessages([]);
    }
    setMeta((prev) => ({
      ...prev,
      sessionId: undefined,
      agentName: undefined,
      agentTitle: undefined,
      agentVersion: undefined,
    }));
  }, [
    clearActiveTranscriptCacheWriteTimer,
    clearReplayCacheContext,
    clearResumeReplayDoneTimer,
    updateResumeReplay,
  ]);

  const resumeSession = useCallback(
    ({ agentId, profileId, launchSession }: ResumeChatSessionRequest) => {
      if (!wsRef.current || wsRef.current.readyState !== WebSocket.OPEN) return false;

      clearConversationView({
        abortReplay: true,
        preserveMessages: true,
        sendStop: false,
      });
      const requestId = resumeRequestIdRef.current + 1;
      resumeRequestIdRef.current = requestId;
      ignoredReplaySessionsRef.current.delete(launchSession.session_id);
      const replay: ResumeReplayState = {
        sessionId: launchSession.session_id,
        title: launchSession.title,
        agentId,
        workspace: launchSession.workspace,
        updatedAt: launchSession.updated_at,
        blocking: true,
      };
      replayMessageBufferRef.current = {
        sessionId: launchSession.session_id,
        messages: [],
      };
      replayCacheContextRef.current = replay;
      updateResumeReplay(replay);
      setMeta((prev) => ({ ...prev, sessionId: launchSession.session_id }));

      void (async () => {
        try {
          const cachedMessages = await readCachedChatSession({
            agentId,
            workspace: launchSession.workspace,
            sessionId: launchSession.session_id,
            updatedAt: launchSession.updated_at,
          });
          if (resumeRequestIdRef.current !== requestId) return;
          if (cachedMessages) {
            const backgroundReplay = { ...replay, blocking: false };
            replayCacheContextRef.current = backgroundReplay;
            updateResumeReplay(backgroundReplay);
            const settledCachedMessages = settleStreamActivitiesMessage(cachedMessages);
            setMessages((prev) => {
              const mergedMessages = mergeChatMessageSnapshots(
                prev,
                settledCachedMessages,
              );
              messagesRef.current = mergedMessages;
              return mergedMessages;
            });
          }
        } catch (error) {
          console.warn("[ChatView] failed to read cached session:", error);
        }

        if (resumeRequestIdRef.current !== requestId) return;
        const ws = wsRef.current;
        if (!ws || ws.readyState !== WebSocket.OPEN) {
          replayMessageBufferRef.current = null;
          clearReplayCacheContext();
          updateResumeReplay(null);
          return;
        }

        try {
          const payload: Record<string, unknown> = {
            type: "resume_session",
            agent: agentId,
            sessionId: launchSession.session_id,
            sessionWorkspace: launchSession.workspace,
          };
          if (profileId !== undefined) {
            payload.profileId = profileId;
          }
          ws.send(JSON.stringify(payload));
        } catch (error) {
          console.warn("[ChatView] failed to resume chat session:", error);
          replayMessageBufferRef.current = null;
          clearReplayCacheContext();
          updateResumeReplay(null);
        }
      })();

      return true;
    },
    [clearConversationView, clearReplayCacheContext, updateResumeReplay],
  );

  const stopStreaming = useCallback(() => {
    const ws = wsRef.current;
    const replayContext = resumeReplayRef.current ?? replayCacheContextRef.current;
    const abortedSessionId = replayContext?.sessionId;
    if (ws?.readyState === WebSocket.OPEN) {
      try {
        ws.send(JSON.stringify({ type: "stop" }));
      } catch (error) {
        console.warn("[ChatView] failed to stop chat message:", error);
      }
    }
    if (abortedSessionId) {
      ignoredReplaySessionsRef.current.add(abortedSessionId);
    }
    promptInFlightRef.current = false;
    setStreaming(false);
    setMessages((prev) => settleStreamActivitiesMessage(prev));
    replayMessageBufferRef.current = null;
    cancelReplayOnNextTurnRef.current = false;
    clearReplayCacheContext();
    updateResumeReplay(null);
  }, [clearReplayCacheContext, updateResumeReplay]);

  const sendPermissionResponse = useCallback((requestId: string, optionId: string) => {
    if (!wsRef.current || wsRef.current.readyState !== WebSocket.OPEN) return;
    wsRef.current.send(JSON.stringify({ type: "permission_response", requestId, optionId }));
    setPendingPermissions((prev) =>
      prev.filter((permission) => permission.requestId !== requestId),
    );
  }, []);

  const cancelPermissionRequest = useCallback((requestId: string) => {
    if (!wsRef.current || wsRef.current.readyState !== WebSocket.OPEN) return;
    wsRef.current.send(
      JSON.stringify({ type: "permission_response", requestId, outcome: "cancelled" }),
    );
    setPendingPermissions((prev) =>
      prev.filter((permission) => permission.requestId !== requestId),
    );
  }, []);

  return {
    messages,
    connected,
    streaming,
    meta,
    agents,
    pendingPermissions,
    sessionMode,
    resumeReplay,
    multiAgentTurns,
    subagents,
    subagentMessages,
    lastPromptDoneAt,
    sendMessage,
    resumeSession,
    clearConversationView,
    setSessionMode,
    setSessionConfigOption,
    stopStreaming,
    sendPermissionResponse,
    cancelPermissionRequest,
  };
}

function applySubagentAcpNotification(
  prev: ChatMessage[],
  notif: SessionNotification,
): ChatMessage[] {
  const update = notif.update;
  switch (update.sessionUpdate) {
    case "user_message_chunk":
      return appendUserMessageChunk(prev, update.content, update.messageId, {
        dedupeExistingText: true,
      });
    case "agent_message_chunk":
      return appendStreamAssistantMessage(prev, update.content, update.messageId);
    case "agent_thought_chunk":
      return appendThinkingActivityMessage(prev, update.content, "Thinking");
    case "tool_call":
    case "tool_call_update": {
      const messages = appendToolActivityMessage(prev, update);
      const status = toolActivityStatus(update);
      if (status === "completed" || status === "failed") {
        return clearStreamProgressMessage(messages);
      }
      return setStreamProgressMessage(
        messages,
        `Using tool: ${toolActivityLabel(update)}...`,
        "tool",
      );
    }
    case "plan":
      return appendPlanMessage(prev, update);
    default:
      return prev;
  }
}

function mergeById<T extends { id: string }>(prev: T[], nextItems: T[]): T[] {
  const byId = new Map(prev.map((item) => [item.id, item]));
  for (const item of nextItems) {
    byId.set(item.id, item);
  }
  return Array.from(byId.values());
}

function messageContentBlocks(
  text: string,
  attachments: ChatAttachment[],
): ContentBlock[] {
  const blocks: ContentBlock[] = [];
  if (text) blocks.push({ type: "text", text });
  blocks.push(
    ...attachments.map((attachment) => ({
      type: "resource_link" as const,
      name: attachment.name,
      title: attachment.name,
      mimeType: attachment.mimeType,
      size: attachment.size,
      uri: attachment.uri,
    })),
  );
  return blocks;
}
