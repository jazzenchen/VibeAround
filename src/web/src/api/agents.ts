/**
 * Agents API: fetch enabled agents from backend.
 */

function getBaseUrl(): string {
  if (typeof window === "undefined") return "http://127.0.0.1:5182";
  return window.location.origin;
}

export interface AgentInfo {
  id: string;
  description: string;
}

export interface AgentsConfig {
  agents: AgentInfo[];
  default_agent: string;
}

export async function getAgents(): Promise<AgentsConfig> {
  const res = await fetch(`${getBaseUrl()}/api/agents`);
  if (!res.ok) throw new Error(`GET /api/agents: ${res.status}`);
  return res.json();
}
