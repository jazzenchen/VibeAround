import type {
  ContentBlock,
  Plan,
  ToolCall,
  ToolCallContent,
  ToolCallLocation,
  ToolCallStatus,
} from "@agentclientprotocol/sdk";

export type ChatActivity = {
  id: string;
  kind: "thinking" | "tool";
  label: string;
  detail?: string;
  status?: string;
  active?: boolean;
};

export type ChatContentPart = {
  id: string;
  kind: "content";
  block: ContentBlock;
};

export type ChatThoughtPart = {
  id: string;
  kind: "thought";
  blocks: ContentBlock[];
  active?: boolean;
};

export type ChatToolCallPart = {
  id: string;
  kind: "tool_call";
  toolCallId: string;
  title: string;
  toolKind?: ToolCall["kind"] | null;
  status?: ToolCallStatus | null;
  locations?: ToolCallLocation[] | null;
  content?: ToolCallContent[] | null;
  rawInput?: unknown;
  rawOutput?: unknown;
  active?: boolean;
};

export type ChatPlanPart = {
  id: string;
  kind: "plan";
  plan: Plan;
};

export type ChatMessagePart =
  | ChatContentPart
  | ChatThoughtPart
  | ChatToolCallPart
  | ChatPlanPart;

export type ChatMessage = {
  role: "user" | "assistant";
  parts?: ChatMessagePart[];
  content: string;
  messageId?: string | null;
  optimistic?: boolean;
  progress?: string;
  progressKind?: "thinking" | "tool";
  activities?: ChatActivity[];
  mode?: "standalone" | "stream";
};

export type ChatDisplaySettings = {
  showThinking: boolean;
  showTools: boolean;
};

export type ChatAttachment = {
  id: string;
  name: string;
  mimeType: string;
  size: number;
  uri: string;
};

export type ChatMeta = {
  channelId?: string;
  sessionId?: string;
  agentTitle?: string;
  agentVersion?: string;
  agentName?: string;
  /** From `session_info`: what the route actually runs, as the server sees it. */
  threadId?: string;
  workspacePath?: string;
  agentId?: string;
  profileId?: string;
};

export type SessionModeOption = {
  value: string;
  name: string;
  description?: string | null;
  group?: string | null;
};

export type SessionModeState = {
  source: "config_option" | "session_mode";
  configId?: string | null;
  name?: string | null;
  description?: string | null;
  currentValue: string;
  options: SessionModeOption[];
};

export type ChatSessionSelection =
  | { kind: "current" }
  | { kind: "new" }
  | { kind: "resume"; sessionId: string };

export type PendingPermission = {
  requestId: string;
  request: unknown;
};
