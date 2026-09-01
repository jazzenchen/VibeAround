import { AGENT_IDS, type AgentId } from "@va/client";

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
