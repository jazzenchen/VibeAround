import type {
  AgentInfo,
  LaunchSessionInfo,
  MultiAgentTurn,
  ThreadAgent,
} from "@va/client";
import type {
  ChatMessage,
  ChatMeta,
  PendingPermission,
  SessionModeState,
} from "./chatTypes";
import type {
  ResumeReplayState,
  useWebChatConnection,
} from "./useWebChatConnection";

export interface ChatRuntimeSpec {
  agentId: string;
  profileId?: string;
  workspacePath?: string;
  launchSession?: LaunchSessionInfo;
  title?: string;
  lastPromptAt?: number;
  initialResume?: {
    agentId: string;
    profileId?: string;
    launchSession: LaunchSessionInfo;
  };
}

export interface ChatRuntimeSnapshot {
  messages: ChatMessage[];
  connected: boolean;
  streaming: boolean;
  meta: ChatMeta;
  agents: AgentInfo[];
  pendingPermissions: PendingPermission[];
  sessionMode: SessionModeState | null;
  resumeReplay: ResumeReplayState | null;
  multiAgentTurns: MultiAgentTurn[];
  subagents: ThreadAgent[];
  subagentMessages: Record<string, ChatMessage[]>;
  lastPromptDoneAt?: number;
}

export interface ChatRuntimeActions {
  sendMessage: ReturnType<typeof useWebChatConnection>["sendMessage"];
  resumeSession: ReturnType<typeof useWebChatConnection>["resumeSession"];
  clearConversationView: ReturnType<typeof useWebChatConnection>["clearConversationView"];
  setSessionMode: ReturnType<typeof useWebChatConnection>["setSessionMode"];
  setSessionConfigOption: ReturnType<typeof useWebChatConnection>["setSessionConfigOption"];
  stopStreaming: ReturnType<typeof useWebChatConnection>["stopStreaming"];
  sendPermissionResponse: ReturnType<typeof useWebChatConnection>["sendPermissionResponse"];
  cancelPermissionRequest: ReturnType<typeof useWebChatConnection>["cancelPermissionRequest"];
}

export const EMPTY_RUNTIME_SNAPSHOT: ChatRuntimeSnapshot = {
  messages: [],
  connected: false,
  streaming: false,
  meta: {},
  agents: [],
  pendingPermissions: [],
  sessionMode: null,
  resumeReplay: null,
  multiAgentTurns: [],
  subagents: [],
  subagentMessages: {},
  lastPromptDoneAt: undefined,
};
