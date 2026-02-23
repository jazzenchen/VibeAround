/** Terminal session status. */
export type TerminalStatus = "running" | "idle" | "stopped" | "error";

/** Tool type: determines panel and xterm theme (claude/gemini/codex/generic). */
export type ToolType = "claude" | "gemini" | "codex" | "generic";

export const TOOL_OPTIONS: ToolType[] = ["generic", "claude", "gemini", "codex"];

export interface TerminalSession {
  id: string;
  name: string;
  group: string;
  tool: ToolType;
  status: TerminalStatus;
  command: string;
  cwd: string;
  startedAt: number;
  /** Backend created_at (seconds); optional, for display. */
  createdAt?: number;
}

export interface TerminalGroup {
  id: string;
  label: string;
  color: string;
  sessions: TerminalSession[];
  collapsed?: boolean;
}

export type ViewMode = "tabs" | "grid";

export interface ToolTheme {
  accent: string;
  accentFg: string;
  bg: string;
  headerBg: string;
  borderColor: string;
  label: string;
  cursorColor: string;
  selectionBg: string;
}

export const toolThemes: Record<ToolType, ToolTheme> = {
  claude: {
    accent: "#d97706",
    accentFg: "#fef3c7",
    bg: "#0f0c08",
    headerBg: "#1a1508",
    borderColor: "#d9770640",
    label: "Claude",
    cursorColor: "#d97706",
    selectionBg: "#d9770633",
  },
  gemini: {
    accent: "#3b82f6",
    accentFg: "#dbeafe",
    bg: "#080a10",
    headerBg: "#0c1020",
    borderColor: "#3b82f640",
    label: "Gemini",
    cursorColor: "#3b82f6",
    selectionBg: "#3b82f633",
  },
  codex: {
    accent: "#10b981",
    accentFg: "#d1fae5",
    bg: "#080f0c",
    headerBg: "#0c1a14",
    borderColor: "#10b98140",
    label: "Codex",
    cursorColor: "#10b981",
    selectionBg: "#10b98133",
  },
  generic: {
    accent: "#64748b",
    accentFg: "#e2e8f0",
    bg: "#0c0c14",
    headerBg: "#14141e",
    borderColor: "#64748b40",
    label: "Terminal",
    cursorColor: "#64748b",
    selectionBg: "#64748b33",
  },
};

export const GROUP_COLOR_PRESETS = [
  { name: "Amber", hex: "#d97706" },
  { name: "Violet", hex: "#8b5cf6" },
  { name: "Cyan", hex: "#06b6d4" },
  { name: "Slate", hex: "#64748b" },
] as const;

export function getGroupColor(hex: string) {
  return {
    bg: hex,
    text: `${hex}cc`,
    ring: `${hex}60`,
    tabBg: `${hex}18`,
    lineBg: hex,
  };
}
