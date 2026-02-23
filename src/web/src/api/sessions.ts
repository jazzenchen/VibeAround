/**
 * Sessions API: list, create, delete. Base URL follows current page (works with tunnel).
 */

function getBaseUrl(): string {
  if (typeof window === "undefined") return "http://127.0.0.1:5182";
  return window.location.origin;
}

export interface SessionListItem {
  session_id: string;
  tool: string;
  status: string;
  created_at: number;
  project_path?: string;
}

export interface CreateSessionBody {
  tool: string;
  project_path?: string;
}

export interface CreateSessionResponse {
  session_id: string;
  tool: string;
  created_at: number;
  project_path?: string;
}

export async function getSessions(): Promise<SessionListItem[]> {
  const res = await fetch(`${getBaseUrl()}/api/sessions`);
  if (!res.ok) throw new Error(`GET /api/sessions: ${res.status}`);
  return res.json();
}

export async function createSession(body: CreateSessionBody): Promise<CreateSessionResponse> {
  const res = await fetch(`${getBaseUrl()}/api/sessions`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`POST /api/sessions: ${res.status} ${text}`);
  }
  return res.json();
}

export async function deleteSession(sessionId: string): Promise<void> {
  const res = await fetch(`${getBaseUrl()}/api/sessions/${sessionId}`, { method: "DELETE" });
  if (!res.ok && res.status !== 204) throw new Error(`DELETE /api/sessions: ${res.status}`);
}
