import { AGENT_IDS, type AgentId } from "@va/client";
import type { ToolType } from "@/lib/terminal-types";

export type { AgentId };

export interface AgentDisplayInfo {
  id: AgentId;
  name: string;
}

/** Display names for every `AgentId`. The `Record<AgentId, ...>` shape
 *  means adding an entry to `resources/agents.json` breaks the build here
 *  until the display name is filled in. */
const AGENT_DISPLAY_NAMES: Record<AgentId, string> = {
  "va-agent": "VibeAround Agent",
  claude: "Claude Code",
  gemini: "Gemini CLI",
  opencode: "Opencode",
  codex: "Codex CLI",
  pi: "Pi",
  cursor: "Cursor",
  kiro: "Kiro",
  "qwen-code": "Qwen Code",
};

function isAgentId(value: string): value is AgentId {
  return (AGENT_IDS as readonly string[]).includes(value);
}

export function getAgentDisplayName(agentId: string): string {
  return isAgentId(agentId) ? AGENT_DISPLAY_NAMES[agentId] : agentId;
}

export function getToolDisplayName(tool: string): string {
  const normalized = tool.toLowerCase();
  return isAgentId(normalized) ? AGENT_DISPLAY_NAMES[normalized] : "Terminal";
}

export function agentIdToToolType(agentId: string): ToolType {
  const normalized = agentId.toLowerCase();
  // ToolType is a subset of AgentId (plus "generic"). Widen here and let
  // the theme layer treat any kind it doesn't recognize as "generic".
  if (
    normalized === "claude" ||
    normalized === "codex" ||
    normalized === "gemini" ||
    normalized === "opencode" ||
    normalized === "pi"
  ) {
    return normalized;
  }
  return "generic";
}
