import type { LaunchSessionInfo } from "@va/client";
import { chatSessionKey } from "./chatSessionModel";

const DRAFT_RUNTIME_PREFIX = "draft";
const SESSION_RUNTIME_PREFIX = "session";
const THREAD_RUNTIME_PREFIX = "thread";
const RANDOM_ID_RADIX = 36;

export const INITIAL_RUNTIME_KEY = `${DRAFT_RUNTIME_PREFIX}:initial`;

export function createDraftRuntimeKey(agentId: string) {
  return [
    DRAFT_RUNTIME_PREFIX,
    agentId,
    Date.now(),
    Math.random().toString(RANDOM_ID_RADIX).slice(2),
  ].join(":");
}

export function chatRuntimeKeyForSession(
  session: Pick<
    LaunchSessionInfo,
    "agent_id" | "workspace" | "session_id" | "thread_id"
  >,
) {
  if (session.thread_id) return chatRuntimeKeyForThread(session.thread_id);
  return `${SESSION_RUNTIME_PREFIX}:${chatSessionKey(session)}`;
}

export function chatRuntimeKeyForThread(threadId: string) {
  return `${THREAD_RUNTIME_PREFIX}:${threadId}`;
}

export function chatIdForThread(threadId: string) {
  return `ws_${threadId}`;
}
