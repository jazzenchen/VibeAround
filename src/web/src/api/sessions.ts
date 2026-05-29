/**
 * Sessions API: list, create, delete. Base URL follows current page (works with tunnel).
 */

import {
  browserBaseUrl,
  ChatUploadResponseSchema,
  CreateSessionResponseSchema,
  LaunchSessionListSchema,
  ProfileLaunchOptionsSchema,
  SessionListSchema,
  TmuxSessionsResponseSchema,
  WorkspaceItemSchema,
  WorkspacesResponseSchema,
  type ChatUploadResponse,
  type CreateSessionResponse,
  type LaunchSessionInfo,
  type ProfileLaunchOption,
  type PtyTool,
  type SessionListItem,
  type TmuxSessionsResponse,
  type WorkspaceItem,
  type WorkspacesResponse,
} from "@va/client";

export type {
  ChatUploadResponse,
  CreateSessionResponse,
  LaunchSessionInfo,
  ProfileLaunchOption,
  SessionListItem,
  TmuxSessionsResponse,
  WorkspaceItem,
  WorkspacesResponse,
};

export interface CreateSessionBody {
  tool?: PtyTool;
  profile_id?: string;
  launch_target?: string;
  project_path?: string;
  tmux_session?: string;
  /** "dark" | "light" — sets COLORFGBG in PTY env as fallback for non-OSC programs. */
  theme?: string;
  /** Initial terminal columns (from client fit). Server falls back to 80 if absent. */
  cols?: number;
  /** Initial terminal rows (from client fit). Server falls back to 24 if absent. */
  rows?: number;
}

const CreateWorkspaceResponseSchema = WorkspacesResponseSchema.extend({
  workspace: WorkspaceItemSchema,
});

export type CreateWorkspaceResponse = WorkspacesResponse & {
  workspace: WorkspaceItem;
};

export async function getSessions(): Promise<SessionListItem[]> {
  const res = await fetch(`${browserBaseUrl()}/api/sessions`);
  if (!res.ok) throw new Error(`GET /api/sessions: ${res.status}`);
  return SessionListSchema.parse(await res.json());
}

export async function getProfiles(): Promise<ProfileLaunchOption[]> {
  const res = await fetch(`${browserBaseUrl()}/api/profiles`);
  if (!res.ok) throw new Error(`GET /api/profiles: ${res.status}`);
  return ProfileLaunchOptionsSchema.parse(await res.json());
}

export async function getWorkspaces(): Promise<WorkspacesResponse> {
  const res = await fetch(`${browserBaseUrl()}/api/workspaces`);
  if (!res.ok) throw new Error(`GET /api/workspaces: ${res.status}`);
  return WorkspacesResponseSchema.parse(await res.json());
}

export async function createWorkspace(name: string): Promise<CreateWorkspaceResponse> {
  const res = await fetch(`${browserBaseUrl()}/api/workspaces/create`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name }),
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(text || `Failed to create workspace (${res.status})`);
  }
  return CreateWorkspaceResponseSchema.parse(await res.json());
}

export async function getLaunchSessions(
  agentId: string,
  includeArchived = false,
  workspacePath?: string,
): Promise<LaunchSessionInfo[]> {
  const params = new URLSearchParams();
  if (includeArchived) params.set("include_archived", "true");
  if (workspacePath) params.set("workspace_path", workspacePath);
  const query = params.toString();
  const path = `/api/agents/${encodeURIComponent(agentId)}/launch-sessions${query ? `?${query}` : ""}`;
  const res = await fetch(`${browserBaseUrl()}${path}`);
  if (!res.ok) throw new Error(`GET ${path}: ${res.status}`);
  return LaunchSessionListSchema.parse(await res.json());
}

export async function getLaunchSessionsBatch(
  agentIds: string[],
  includeArchived = false,
  workspacePaths: string[] = [],
): Promise<LaunchSessionInfo[]> {
  if (agentIds.length === 0) return [];
  const path = "/api/launch-sessions";
  const res = await fetch(`${browserBaseUrl()}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      agent_ids: agentIds,
      workspace_paths: workspacePaths,
      include_archived: includeArchived,
    }),
  });
  if (!res.ok) throw new Error(`POST ${path}: ${res.status}`);
  return LaunchSessionListSchema.parse(await res.json());
}

export async function archiveLaunchSession(
  agentId: string,
  sessionId: string,
  workspacePath: string,
): Promise<void> {
  const path = `/api/agents/${encodeURIComponent(agentId)}/launch-sessions/${encodeURIComponent(
    sessionId,
  )}/archive`;
  const res = await fetch(`${browserBaseUrl()}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ workspace_path: workspacePath }),
  });
  if (!res.ok && res.status !== 204) throw new Error(`POST ${path}: ${res.status}`);
}

export async function unarchiveLaunchSession(
  agentId: string,
  sessionId: string,
  workspacePath: string,
): Promise<void> {
  const path = `/api/agents/${encodeURIComponent(agentId)}/launch-sessions/${encodeURIComponent(
    sessionId,
  )}/unarchive`;
  const res = await fetch(`${browserBaseUrl()}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ workspace_path: workspacePath }),
  });
  if (!res.ok && res.status !== 204) throw new Error(`POST ${path}: ${res.status}`);
}

export async function createSession(body: CreateSessionBody): Promise<CreateSessionResponse> {
  const res = await fetch(`${browserBaseUrl()}/api/sessions`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`POST /api/sessions: ${res.status} ${text}`);
  }
  return CreateSessionResponseSchema.parse(await res.json());
}

export async function deleteSession(sessionId: string): Promise<void> {
  const res = await fetch(`${browserBaseUrl()}/api/sessions/${sessionId}`, { method: "DELETE" });
  if (!res.ok && res.status !== 204) throw new Error(`DELETE /api/sessions: ${res.status}`);
}

export async function getTmuxSessions(): Promise<TmuxSessionsResponse> {
  const res = await fetch(`${browserBaseUrl()}/api/tmux/sessions`);
  if (!res.ok) throw new Error(`GET /api/tmux/sessions: ${res.status}`);
  return TmuxSessionsResponseSchema.parse(await res.json());
}

export async function uploadChatFile(file: File): Promise<ChatUploadResponse> {
  const params = new URLSearchParams();
  const mimeType = uploadMimeType(file);
  params.set("filename", file.name || "attachment");
  if (mimeType) params.set("mime_type", mimeType);
  const res = await fetch(`${browserBaseUrl()}/api/chat/uploads?${params.toString()}`, {
    method: "POST",
    headers: mimeType ? { "Content-Type": mimeType } : undefined,
    body: file,
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`POST /api/chat/uploads: ${res.status} ${text}`);
  }
  return ChatUploadResponseSchema.parse(await res.json());
}

function uploadMimeType(file: File): string | undefined {
  const mime = file.type.split(";")[0]?.trim();
  if (!mime || isGenericUploadMime(mime)) return undefined;
  return mime;
}

function isGenericUploadMime(mime: string): boolean {
  const normalized = mime.toLowerCase();
  return normalized === "application/octet-stream" || normalized === "binary/octet-stream";
}
